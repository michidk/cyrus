// Optional diagnostic tap at the legacy SDL mixer-output callback.
// Copy on the audio producer; perform filesystem delivery on a bounded worker.
#define _GNU_SOURCE
#include <SDL.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define SLOTS 64
#define BLOCK_BYTES 65536
struct block { uint64_t sequence, first, last, ns; int length; unsigned char pcm[BLOCK_BYTES]; };
static struct block ring[SLOTS];
static atomic_uint head, tail;
static atomic_bool running, failed;
static atomic_ullong revision, frames, sequence, generated_ns;
static SDL_AudioCallback callback;
static void *callback_data;
static unsigned rate;
static FILE *pcm, *events;
static pthread_t worker;
static int active;

// The presentation owner reads one committed callback prefix, not writer progress.
int cyrus_mixer_cursor(uint64_t *out_frames, uint64_t *out_sequence, uint64_t *out_ns, unsigned *out_rate) {
    uint64_t before = atomic_load_explicit(&revision, memory_order_acquire);
    if (!active || (before & 1) || atomic_load(&failed)) return -1;
    *out_frames = atomic_load(&frames);
    *out_sequence = atomic_load(&sequence);
    *out_ns = atomic_load(&generated_ns);
    *out_rate = rate;
    return before == atomic_load_explicit(&revision, memory_order_acquire) && *out_sequence ? 0 : -1;
}

static void *deliver(void *unused) {
    (void)unused;
    while (atomic_load(&running) || atomic_load(&tail) != atomic_load(&head)) {
        unsigned read = atomic_load_explicit(&tail, memory_order_relaxed);
        if (read == atomic_load_explicit(&head, memory_order_acquire)) {
            struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL); continue;
        }
        struct block *block = &ring[read % SLOTS];
        if (fwrite(block->pcm, 1, block->length, pcm) != (size_t)block->length || fflush(pcm) ||
            fprintf(events, "{\"sequence\":%llu,\"first\":%llu,\"last\":%llu,\"monotonic_ns\":%llu,\"rate\":%u}\n",
                (unsigned long long)block->sequence, (unsigned long long)block->first,
                (unsigned long long)block->last, (unsigned long long)block->ns, rate) < 0 || fflush(events))
            atomic_store(&failed, 1);
        atomic_store_explicit(&tail, read + 1, memory_order_release);
    }
    return NULL;
}

static void SDLCALL generate(void *unused, Uint8 *stream, int length) {
    (void)unused;
    callback(callback_data, stream, length);
    struct timespec now; clock_gettime(CLOCK_MONOTONIC, &now);
    uint64_t ns = (uint64_t)now.tv_sec * 1000000000ULL + now.tv_nsec;
    unsigned write = atomic_load_explicit(&head, memory_order_relaxed);
    uint64_t first = atomic_load(&frames), next = atomic_load(&sequence) + 1;
    if (length <= 0 || length > BLOCK_BYTES || length % 4 ||
        write - atomic_load_explicit(&tail, memory_order_acquire) == SLOTS) {
        atomic_store(&failed, 1);
        return;
    }
    struct block *block = &ring[write % SLOTS];
    block->sequence = next; block->first = first; block->last = first + (unsigned)length / 4;
    block->ns = ns; block->length = length; memcpy(block->pcm, stream, length);
    atomic_fetch_add_explicit(&revision, 1, memory_order_acq_rel);
    atomic_store(&frames, block->last); atomic_store(&sequence, next); atomic_store(&generated_ns, ns);
    atomic_fetch_add_explicit(&revision, 1, memory_order_release);
    atomic_store_explicit(&head, write + 1, memory_order_release);
}

static void stop(void) {
    if (!active) return;
    active = 0;
    atomic_store(&running, 0); pthread_join(worker, NULL);
    if (atomic_load(&failed)) fputs("{\"error\":\"generated PCM discontinuity or delivery failure\"}\n", events);
    fclose(pcm); fclose(events);
}

int SDL_OpenAudio(SDL_AudioSpec *desired, SDL_AudioSpec *obtained) {
    int (*open_audio)(SDL_AudioSpec *, SDL_AudioSpec *);
    *(void **)(&open_audio) = dlsym(RTLD_NEXT, "SDL_OpenAudio");
    if (!open_audio) return -1;
    const char *directory = getenv("CYRUS_MIXER_CAPTURE_DIR");
    if (!directory) return open_audio(desired, obtained);
    if (active || !desired || !desired->callback) return SDL_SetError("mixer capture requires one callback device");
    char path[4096];
    if (strlen(directory) > sizeof(path) - 32) return SDL_SetError("mixer capture path too long");
    snprintf(path, sizeof(path), "%s/generated.raw", directory); pcm = fopen(path, "wx");
    snprintf(path, sizeof(path), "%s/generated.jsonl", directory); events = fopen(path, "wx");
    if (!pcm || !events) {
        if (pcm) fclose(pcm);
        if (events) fclose(events);
        return SDL_SetError("mixer capture output unavailable or already exists");
    }
    callback = desired->callback; callback_data = desired->userdata;
    SDL_AudioSpec request = *desired, actual;
    request.callback = generate; request.userdata = NULL;
    int result = open_audio(&request, &actual);
    if (result != 0) { fclose(pcm); fclose(events); return result; }
    if (actual.format != AUDIO_S16LSB || actual.channels != 2 || actual.freq <= 0 ||
        actual.size > BLOCK_BYTES) {
        void (*close_audio)(void); *(void **)(&close_audio) = dlsym(RTLD_NEXT, "SDL_CloseAudio");
        if (close_audio) close_audio();
        fclose(pcm); fclose(events);
        return SDL_SetError("mixer capture requires bounded stereo s16le PCM");
    }
    rate = (unsigned)actual.freq;
    atomic_store(&head, 0); atomic_store(&tail, 0); atomic_store(&frames, 0);
    atomic_store(&sequence, 0); atomic_store(&generated_ns, 0); atomic_store(&revision, 0);
    atomic_store(&failed, 0); atomic_store(&running, 1);
    if (pthread_create(&worker, NULL, deliver, NULL) != 0) {
        void (*close_audio)(void); *(void **)(&close_audio) = dlsym(RTLD_NEXT, "SDL_CloseAudio");
        if (close_audio) close_audio();
        fclose(pcm); fclose(events);
        return SDL_SetError("mixer capture worker unavailable");
    }
    active = 1;
    if (obtained) *obtained = actual;
    return 0;
}

void SDL_CloseAudio(void) {
    void (*close_audio)(void); *(void **)(&close_audio) = dlsym(RTLD_NEXT, "SDL_CloseAudio");
    if (close_audio) close_audio();
    stop();
}

__attribute__((destructor)) static void finish(void) { if (active) SDL_CloseAudio(); }
