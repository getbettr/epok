ARG HUB=docker.io

FROM ${HUB}/library/debian:bullseye-slim
RUN apt-get update && apt-get install -y openssh-client libssl-dev && rm -rf /var/lib/apt/lists/*

ARG CACHE_BUST=no
RUN echo ${CACHE_BUST}

COPY target/release/epok /usr/local/bin/
COPY target/release/epok-clean /usr/local/bin/

RUN useradd -u 1000 -U epok
COPY ./docker/yolo-ssh /home/epok/.ssh/config
RUN chown -R epok: /home/epok

USER epok
ENV HOME=/home/epok

CMD ["epok"]
