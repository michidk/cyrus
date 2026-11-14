# Runs the original DOS game (GOG build) under DOSBox-X with its built-in
# Voodoo emulation, on a virtual X display served to a browser over noVNC.
FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        dosbox-x xvfb x11vnc novnc websockify x11-utils xdotool scrot python3-pil \
        libgl1-mesa-dri mesa-utils ca-certificates pulseaudio pulseaudio-utils libasound2-plugins \
    && rm -rf /var/lib/apt/lists/*
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY dosbox-x.conf /etc/cyrus/dosbox-x.conf
RUN chmod +x /usr/local/bin/entrypoint.sh
EXPOSE 6080
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
