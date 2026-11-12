// Synthetic public SDL caller: mutate clock and pixels immediately after present.
#include <SDL.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#ifdef FAIL_PRESENT_BACKEND
int SDL_UpdateWindowSurfaceRects(SDL_Window *window, const SDL_Rect *rectangles, int count) {
    (void)window; (void)rectangles; (void)count;
    return SDL_SetError("injected backend presentation failure");
}
#else
int main(int argc, char **argv) {
    if (argc != 3 || SDL_Init(SDL_INIT_VIDEO) != 0) return 1;
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
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}

#endif
