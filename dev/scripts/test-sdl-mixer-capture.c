#define _POSIX_C_SOURCE 200809L
// Public SDL callback produces known stereo PCM. Writer delivery is asynchronous.
#include <SDL.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#ifdef STALL_DELIVERY
size_t fwrite(const void *pointer, size_t size, size_t count, FILE *stream) {
    size_t (*write_bytes)(const void *, size_t, size_t, FILE *);
    *(void **)(&write_bytes) = dlsym(RTLD_NEXT, "fwrite");
    struct timespec delay = {0, 40000000}; nanosleep(&delay, NULL);
    return write_bytes(pointer, size, count, stream);
}
#else
static void sound(void *unused, Uint8 *stream, int length) {
    (void)unused;
    int16_t *samples = (int16_t *)stream;
    for (int i = 0; i < length / 2; i++) samples[i] = i % 2 ? -1234 : 1234;
}
int main(int argc, char **argv) {
    if (SDL_Init(SDL_INIT_AUDIO)) return 1;
    SDL_AudioSpec desired = {0}, actual;
    desired.freq = 48000; desired.format = AUDIO_S16LSB; desired.channels = argc > 1 && strstr(argv[1], "unsupported") != NULL ? 1 : 2;
    desired.samples = 512; desired.callback = sound;
    int device_api = argc > 1 && !strncmp(argv[1], "device", 6);
    SDL_AudioDeviceID device = 0;
    if (device_api) {
        device = SDL_OpenAudioDevice(NULL, 0, &desired, &actual, 0);
        if (!device) return 2;
        SDL_PauseAudioDevice(device, 0);
    } else {
        if (SDL_OpenAudio(&desired, &actual)) return 2;
        SDL_PauseAudio(0);
    }
    SDL_Delay(argc > 1 && strstr(argv[1], "overflow") != NULL ? 1500 : 300);
    int (*cursor)(uint64_t *, uint64_t *, uint64_t *, unsigned *);
    *(void **)(&cursor) = dlsym(RTLD_DEFAULT, "cyrus_mixer_cursor");
    uint64_t frames, sequence, ns; unsigned rate;
    if (!cursor || cursor(&frames, &sequence, &ns, &rate)) return 3;
    char path[4096]; struct stat delivered;
    snprintf(path, sizeof(path), "%s/generated.raw", getenv("CYRUS_MIXER_CAPTURE_DIR"));
    if (stat(path, &delivered)) return 4;
    printf("{\"delivered_frames\":%llu,\"frames\":%llu,\"sequence\":%llu,\"monotonic_ns\":%llu,\"rate\":%u}\n",
        (unsigned long long)delivered.st_size / 4, (unsigned long long)frames, (unsigned long long)sequence, (unsigned long long)ns, rate);
    if (device_api) SDL_CloseAudioDevice(device); else SDL_CloseAudio();
    SDL_Quit();
    return 0;
}

#endif
