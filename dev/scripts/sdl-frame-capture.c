// Optional diagnostic interposer for SDL2 software surface presentation.
// Bindings are supplied by the local probe; this file contains no game layout.
#define _GNU_SOURCE
#include <SDL.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define FIELD_LIMIT 16
#define BYTE_LIMIT 4096
#define PATH_LIMIT 4096
struct field { char name[64]; uint64_t address; size_t size; unsigned char bytes[BYTE_LIMIT]; };
struct capture {
    char output[PATH_LIMIT];
    char error[160];
    char pcm_path[PATH_LIMIT];
    long long pcm_frames;
    struct field fields[FIELD_LIMIT];
    size_t count;
    uint64_t sequence;
    long thread;
    struct timespec time;
    SDL_Surface *image;
};
static atomic_bool busy = 0;
static uint64_t sequence = 0;
static pthread_mutex_t owner = PTHREAD_MUTEX_INITIALIZER;

static void *write_capture(void *pointer) {
    struct capture *capture = pointer;
    char path[PATH_LIMIT + 32];
    if (capture->image) {
        snprintf(path, sizeof(path), "%s/frame.bmp", capture->output);
        if (SDL_SaveBMP(capture->image, path) != 0)
            snprintf(capture->error, sizeof(capture->error), "SDL BMP write failed: %.110s", SDL_GetError());
        SDL_FreeSurface(capture->image);
    }
    snprintf(path, sizeof(path), "%s/result.tmp", capture->output);
    FILE *result = fopen(path, "wx");
    if (result) {
        fprintf(result, "{\"owner\":\"sdl_surface_present\",\"sequence\":%llu,\"thread\":%ld,\"monotonic_ns\":%llu,\"ok\":%s,\"pcm_delivery_frames\":%lld,\"fields\":{",
            (unsigned long long)capture->sequence, capture->thread,
            (unsigned long long)capture->time.tv_sec * 1000000000ULL + capture->time.tv_nsec,
            capture->error[0] ? "false" : "true", capture->pcm_frames);
        for (size_t i = 0; i < capture->count; i++) {
            fprintf(result, "%s\"%s\":\"", i ? "," : "", capture->fields[i].name);
            for (size_t j = 0; j < capture->fields[i].size; j++) fprintf(result, "%02x", capture->fields[i].bytes[j]);
            fputc('"', result);
        }
        // Errors are fixed diagnostic messages, never raw paths or JSON text.
        fprintf(result, "},\"error\":\"%s\"}\n", capture->error[0] ? "surface or binding capture failed" : "");
        if (fclose(result) == 0) {
            char target[PATH_LIMIT + 32];
            snprintf(target, sizeof(target), "%s/result.json", capture->output);
            rename(path, target);
        }
    }
    free(capture);
    atomic_store_explicit(&busy, 0, memory_order_release);
    return NULL;
}

static int read_fields(struct capture *capture, int verify) {
    int memory = open("/proc/self/mem", O_RDONLY | O_CLOEXEC);
    if (memory < 0) return -1;
    int result = 0;
    for (size_t i = 0; i < capture->count; i++) {
        struct field *field = &capture->fields[i];
        unsigned char bytes[BYTE_LIMIT];
        if (pread(memory, bytes, field->size, (off_t)field->address) != (ssize_t)field->size ||
            (verify && memcmp(bytes, field->bytes, field->size) != 0)) { result = -1; break; }
        if (!verify) memcpy(field->bytes, bytes, field->size);
    }
    close(memory);
    return result;
}

static struct capture *freeze_surface(SDL_Window *window) {
    const char *directory = getenv("CYRUS_FRAME_CAPTURE_DIR");
    if (!directory || pthread_mutex_trylock(&owner) != 0) return NULL;
    uint64_t current = ++sequence;
    if (atomic_load_explicit(&busy, memory_order_acquire)) { pthread_mutex_unlock(&owner); return NULL; }
    char request[PATH_LIMIT + 32];
    if (strlen(directory) >= PATH_LIMIT) { pthread_mutex_unlock(&owner); return NULL; }
    snprintf(request, sizeof(request), "%s/request", directory);
    FILE *input = fopen(request, "r");
    if (!input) { pthread_mutex_unlock(&owner); return NULL; }
    struct capture *capture = calloc(1, sizeof(*capture));
    if (!capture) { fclose(input); pthread_mutex_unlock(&owner); return NULL; }
    capture->pcm_frames = -1;
    capture->sequence = current;
    capture->thread = syscall(SYS_gettid);
    clock_gettime(CLOCK_MONOTONIC, &capture->time);
    if (!fgets(capture->output, sizeof(capture->output), input)) capture->error[0] = 1;
    capture->output[strcspn(capture->output, "\r\n")] = 0;
    if (capture->output[0] != '/' || capture->error[0]) {
        fclose(input); unlink(request); free(capture); pthread_mutex_unlock(&owner); return NULL;
    }
    char line[256];
    while (fgets(line, sizeof(line), input)) {
        if (!strncmp(line, "pcm ", 4)) {
            line[strcspn(line, "\r\n")] = 0;
            if (capture->pcm_path[0] || line[4] != '/') capture->error[0] = 1;
            snprintf(capture->pcm_path, sizeof(capture->pcm_path), "%s", line + 4);
            continue;
        }
        if (capture->count == FIELD_LIMIT) { capture->error[0] = 1; break; }
        struct field *field = &capture->fields[capture->count];
        unsigned long long address;
        char extra;
        if (sscanf(line, "%63[a-zA-Z0-9_] %llu %zu %c", field->name, &address, &field->size, &extra) != 3 ||
            !field->size || field->size > BYTE_LIMIT || address > INT64_MAX) { capture->error[0] = 1; break; }
        field->address = address;
        for (size_t i = 0; i < capture->count; i++)
            if (!strcmp(field->name, capture->fields[i].name)) capture->error[0] = 1;
        capture->count++;
    }
    fclose(input);
    unlink(request);
    if (!capture->count) capture->error[0] = 1;
    if (capture->pcm_path[0]) {
        struct stat pcm;
        if (stat(capture->pcm_path, &pcm) == 0 && S_ISREG(pcm.st_mode))
            capture->pcm_frames = pcm.st_size / 4;
        else capture->error[0] = 1;
    }
    SDL_Surface *surface = SDL_GetWindowSurface(window);
    if (!capture->error[0] && surface && surface->w > 0 && surface->h > 0 &&
        (uint64_t)surface->w * surface->h <= 4096 * 4096 && read_fields(capture, 0) == 0) {
        capture->image = SDL_ConvertSurfaceFormat(surface, SDL_PIXELFORMAT_RGB24, 0);
        if (!capture->image || read_fields(capture, 1) != 0) capture->error[0] = 1;
    } else capture->error[0] = 1;
    atomic_store_explicit(&busy, 1, memory_order_release);
    pthread_mutex_unlock(&owner);
    return capture;
}

static void complete_surface(struct capture *capture, int presented) {
    if (!capture) return;
    if (presented != 0) capture->error[0] = 1;
    pthread_t worker;
    if (pthread_create(&worker, NULL, write_capture, capture) == 0) pthread_detach(worker);
    else {
        // Never perform encoder or filesystem output on the presentation thread.
        if (capture->image) SDL_FreeSurface(capture->image);
        free(capture);
        atomic_store_explicit(&busy, 0, memory_order_release);
    }
}

int SDL_UpdateWindowSurface(SDL_Window *window) {
    int (*present)(SDL_Window *);
    *(void **)(&present) = dlsym(RTLD_NEXT, "SDL_UpdateWindowSurface");
    if (!present) return -1;
    struct capture *capture = freeze_surface(window);
    int result = present(window);
    complete_surface(capture, result);
    return result;
}

int SDL_UpdateWindowSurfaceRects(SDL_Window *window, const SDL_Rect *rectangles, int count) {
    int (*present)(SDL_Window *, const SDL_Rect *, int);
    *(void **)(&present) = dlsym(RTLD_NEXT, "SDL_UpdateWindowSurfaceRects");
    if (!present) return -1;
    struct capture *capture = freeze_surface(window);
    int result = present(window, rectangles, count);
    complete_surface(capture, result);
    return result;
}
