FROM rust:1.61.0-bullseye as builder
WORKDIR /usr/src/epok
COPY ./src ./src
COPY Cargo.lock Cargo.toml build.rs ./
RUN cargo install --path .

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y openssh-client libssl-dev && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/epok /usr/local/bin/epok

RUN useradd -u 1000 -U epok
COPY ./docker/ssh-config /home/epok/.ssh/config
RUN chown -R epok: /home/epok

USER epok
ENV HOME=/home/epok

CMD ["epok"]
