# Runtime for cyrus without a GPU or screen: Xvfb + lavapipe (software Vulkan) + noVNC.
# The binary is built on the host and bind-mounted; this image only has the libraries.
FROM ubuntu:26.04@sha256:6a06111c3b0a8600feadb270aec771dc239bf407b25625f93f44ab00e11f4d3c
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        xvfb x11vnc novnc websockify xdotool mesa-vulkan-drivers vulkan-tools \
        libasound2t64 libudev1 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcursor1 libxi6 libxrandr2 \
        ca-certificates zenity pulseaudio pulseaudio-utils libasound2-plugins \
    && rm -rf /var/lib/apt/lists/*
COPY entrypoint.sh paced-audio.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/entrypoint.sh /usr/local/bin/paced-audio.sh
EXPOSE 6080 7117
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
