// Synthetic public SDL caller: mutate clock and pixels immediately after present.
#include <SDL.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifdef FAIL_PRESENT_BACKEND
int SDL_UpdateWindowSurfaceRects(SDL_Window *window, const SDL_Rect *rectangles, int count) {
    (void)window; (void)rectangles; (void)count;
    return SDL_SetError("injected backend presentation failure");
}
#else
static void generated_audio(void *unused, Uint8 *stream, int length) {
    (void)unused; memset(stream, 17, length);
}
int main(int argc, char **argv) {
    if (argc != 3) return 1;
    int audio = !strcmp(argv[2], "generated");
    if (SDL_Init(SDL_INIT_VIDEO | (audio ? SDL_INIT_AUDIO : 0)) != 0) return 1;
    if (audio) {
        SDL_AudioSpec desired = {0};
        desired.freq = 48000; desired.format = AUDIO_S16LSB; desired.channels = 2;
        desired.samples = 512; desired.callback = generated_audio;
        if (SDL_OpenAudio(&desired, NULL)) return 6;
        SDL_PauseAudio(0); SDL_Delay(100);
    }
    SDL_Window *window = SDL_CreateWindow("capture control", 0, 0, 16, 16, 0);
    SDL_Surface *surface = SDL_GetWindowSurface(window);
    if (!surface) return 2;
    uint32_t clock = 123;
    char path[4096];
    snprintf(path, sizeof(path), "%s/request", getenv("CYRUS_FRAME_CAPTURE_DIR"));
    FILE *request = fopen(path, "w");
    if (!request) return 3;
    fprintf(request, "%s\nclock %llu 4\n", argv[1],
        argv[2][0] != 'i' ? (unsigned long long)(uintptr_t)&clock : 1ULL);
    fclose(request);
    SDL_FillRect(surface, NULL, SDL_MapRGB(surface->format, 255, 0, 0));
    SDL_Rect rectangle = {0, 0, 16, 16};
    int presented = argv[2][0] == 'f'
        ? SDL_UpdateWindowSurfaceRects(window, NULL, -1)
        : argv[2][0] == 'r'
        ? SDL_UpdateWindowSurfaceRects(window, &rectangle, 1)
        : SDL_UpdateWindowSurface(window);
    if ((argv[2][0] == 'f') != (presented != 0)) return 4;
    clock = 999;
    SDL_FillRect(surface, NULL, SDL_MapRGB(surface->format, 0, 0, 255));
    snprintf(path, sizeof(path), "%s/result.json", argv[1]);
    for (int i = 0; i < 500 && access(path, F_OK) != 0; i++) SDL_Delay(10);
    if (access(path, F_OK) != 0) return 5;
    if (audio) SDL_CloseAudio();
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}

#endif
