# Runs the original DOS game (GOG build) under DOSBox-X with its built-in
# Voodoo emulation, on a virtual X display served to a browser over noVNC.
FROM ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db
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
