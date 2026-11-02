# Runs the original DOS game (GOG build) under DOSBox-X with its built-in
# Voodoo emulation, on a virtual X display served to a browser over noVNC.
FROM ubuntu:26.04@sha256:6a06111c3b0a8600feadb270aec771dc239bf407b25625f93f44ab00e11f4d3c
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
