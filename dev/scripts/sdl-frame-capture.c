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
struct field {
    char name[64];
    uint64_t address;
    size_t size;
    int indirect;
    size_t source, offset, width;
    int64_t addend;
    unsigned char bytes[BYTE_LIMIT];
};
struct capture {
    char output[PATH_LIMIT];
    char error[160];
    const char *rejection;
    char pcm_path[PATH_LIMIT];
    long long pcm_frames;
    uint64_t generated_frames, generated_sequence, generated_ns;
    unsigned generated_rate;
    int generated;
    struct field fields[FIELD_LIMIT];
    size_t count;
    char clock_field[64];
    uint64_t clock_value, expires_ns;
    uint64_t sequence;
    long thread;
    struct timespec time;
    Uint32 pixel_format, rmask, gmask, bmask, amask;
    int width, height, pitch, bits_per_pixel;
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
        if (capture->generated)
            fprintf(result, "},\"pcm_generation\":{\"frames\":%llu,\"sequence\":%llu,\"monotonic_ns\":%llu,\"rate\":%u",
                (unsigned long long)capture->generated_frames, (unsigned long long)capture->generated_sequence,
                (unsigned long long)capture->generated_ns, capture->generated_rate);
        fprintf(result, "},\"surface\":{\"format\":%u,\"width\":%d,\"height\":%d,\"pitch\":%d,\"bits_per_pixel\":%d,\"masks\":[%u,%u,%u,%u]",
            capture->pixel_format, capture->width, capture->height, capture->pitch,
            capture->bits_per_pixel, capture->rmask, capture->gmask, capture->bmask, capture->amask);
        // Errors are fixed diagnostic messages, never raw paths or JSON text.
        fprintf(result, "},\"error\":\"%s\"}\n", capture->error[0] ? (capture->rejection ? capture->rejection : "surface or binding capture failed") : "");
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
        if (field->indirect) {
            const unsigned char *source = capture->fields[field->source].bytes + field->offset;
            uint64_t pointer = 0;
            for (size_t j = 0; j < field->width; j++) pointer |= (uint64_t)source[j] << (8 * j);
            if (!pointer || pointer > INT64_MAX) { result = -1; break; }
            if (field->addend < 0) {
                uint64_t magnitude = (uint64_t)(-(field->addend + 1)) + 1;
                if (pointer <= magnitude) { result = -1; break; }
                field->address = pointer - magnitude;
            } else {
                if (pointer > INT64_MAX - (uint64_t)field->addend) { result = -1; break; }
                field->address = pointer + field->addend;
            }
        }
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
        if (!strncmp(line, "@clock ", 7)) {
            unsigned long long value, expires;
            char extra;
            if (capture->clock_field[0] || sscanf(line, "@clock %63[a-zA-Z0-9_] %llu %llu %c",
                    capture->clock_field, &value, &expires, &extra) != 3 || value > UINT32_MAX || !expires)
                capture->error[0] = 1;
            else { capture->clock_value = value; capture->expires_ns = expires; }
            continue;
        }
        if (!strncmp(line, "pcm ", 4)) {
            line[strcspn(line, "\r\n")] = 0;
            if (capture->pcm_path[0] || line[4] != '/') capture->error[0] = 1;
            snprintf(capture->pcm_path, sizeof(capture->pcm_path), "%s", line + 4);
            continue;
        }
        if (capture->count == FIELD_LIMIT) { capture->error[0] = 1; break; }
        struct field *field = &capture->fields[capture->count];
        char extra;
        if (!strncmp(line, "@pointer ", 9)) {
            char source[64];
            long long addend;
            if (sscanf(line, "@pointer %63[a-zA-Z0-9_] %63[a-zA-Z0-9_] %zu %zu %lld %zu %c",
                    field->name, source, &field->offset, &field->width, &addend, &field->size, &extra) != 6 ||
                    !field->size || field->size > BYTE_LIMIT || (field->width != 4 && field->width != 8)) {
                capture->error[0] = 1; break;
            }
            field->source = capture->count;
            for (size_t i = 0; i < capture->count; i++)
                if (!strcmp(source, capture->fields[i].name)) field->source = i;
            if (field->source == capture->count || field->offset > capture->fields[field->source].size ||
                    field->width > capture->fields[field->source].size - field->offset) {
                capture->error[0] = 1; break;
            }
            field->addend = addend;
            field->indirect = 1;
        } else {
            unsigned long long address;
            if (sscanf(line, "%63[a-zA-Z0-9_] %llu %zu %c", field->name, &address, &field->size, &extra) != 3 ||
                    !field->size || field->size > BYTE_LIMIT || address > INT64_MAX) {
                capture->error[0] = 1; break;
            }
            field->address = address;
        }
        for (size_t i = 0; i < capture->count; i++)
            if (!strcmp(field->name, capture->fields[i].name)) capture->error[0] = 1;
        capture->count++;
    }
    fclose(input);
    if (!capture->error[0] && capture->clock_field[0]) {
        struct field *clock = NULL;
        for (size_t i = 0; i < capture->count; i++)
            if (!strcmp(capture->fields[i].name, capture->clock_field)) clock = &capture->fields[i];
        uint64_t now = (uint64_t)capture->time.tv_sec * 1000000000ULL + capture->time.tv_nsec;
        if (now >= capture->expires_ns) {
            capture->error[0] = 1; capture->rejection = "scheduled presentation deadline expired";
        } else if (!clock || clock->size != 4 || read_fields(capture, 0))
            capture->error[0] = 1;
        else {
            uint64_t value = (uint64_t)clock->bytes[0] | (uint64_t)clock->bytes[1] << 8 |
                (uint64_t)clock->bytes[2] << 16 | (uint64_t)clock->bytes[3] << 24;
            if (value < capture->clock_value) {
                free(capture); pthread_mutex_unlock(&owner); return NULL;
            }
            if (value != capture->clock_value) {
                capture->error[0] = 1; capture->rejection = "scheduled presentation clock skipped";
            }
        }
    }
    unlink(request);
    if (!capture->count) capture->error[0] = 1;
    if (capture->pcm_path[0]) {
        struct stat pcm;
        if (stat(capture->pcm_path, &pcm) == 0 && S_ISREG(pcm.st_mode))
            capture->pcm_frames = pcm.st_size / 4;
        else capture->error[0] = 1;
    }
    int (*mixer_cursor)(uint64_t *, uint64_t *, uint64_t *, unsigned *);
    *(void **)(&mixer_cursor) = dlsym(RTLD_DEFAULT, "cyrus_mixer_cursor");
    if (getenv("CYRUS_MIXER_CAPTURE_DIR")) {
        if (!mixer_cursor || mixer_cursor(&capture->generated_frames, &capture->generated_sequence,
                &capture->generated_ns, &capture->generated_rate)) capture->error[0] = 1;
        else capture->generated = 1;
    }
    SDL_Surface *surface = SDL_GetWindowSurface(window);
    if (!capture->error[0] && surface && surface->w > 0 && surface->h > 0 &&
        (uint64_t)surface->w * surface->h <= 4096 * 4096 && read_fields(capture, capture->clock_field[0] ? 1 : 0) == 0) {
        capture->pixel_format = surface->format->format;
        capture->width = surface->w; capture->height = surface->h; capture->pitch = surface->pitch;
        capture->bits_per_pixel = surface->format->BitsPerPixel;
        capture->rmask = surface->format->Rmask; capture->gmask = surface->format->Gmask;
        capture->bmask = surface->format->Bmask; capture->amask = surface->format->Amask;
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
