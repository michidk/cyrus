# Runtime for cyrus without a GPU or screen: Xvfb + lavapipe (software Vulkan) + noVNC.
# The binary is built on the host and bind-mounted; this image only has the libraries.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        xvfb x11vnc novnc websockify xdotool mesa-vulkan-drivers vulkan-tools \
        libasound2t64 libudev1 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcursor1 libxi6 libxrandr2 \
        ca-certificates zenity \
    && rm -rf /var/lib/apt/lists/*
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
EXPOSE 6080 7117
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
