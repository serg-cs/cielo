FROM rust:1.97.1-alpine3.24 AS builder
WORKDIR /usr/src/cielo
COPY Cargo.toml Cargo.lock askama.toml build.rs ./
COPY src ./src
COPY web ./web
RUN cargo build --locked --release

FROM alpine:3.24
RUN apk add --no-cache ca-certificates
COPY --from=builder /usr/src/cielo/target/release/cielo /usr/local/bin/cielo
ENTRYPOINT ["cielo"]
