# Runs the original DOS game (GOG build) under DOSBox-X with its built-in
# Voodoo emulation, on a virtual X display served to a browser over noVNC.
FROM ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        dosbox-x xvfb x11vnc novnc websockify x11-utils xdotool scrot \
        libgl1-mesa-dri mesa-utils ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY dosbox-x.conf /etc/cyrus/dosbox-x.conf
RUN chmod +x /usr/local/bin/entrypoint.sh
EXPOSE 6080
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
