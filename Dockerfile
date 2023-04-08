FROM public.ecr.aws/docker/library/rust:1.68.0-bullseye as build-env

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /app
COPY . /app

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp /app/target/release/sentry-kubernetes /

FROM gcr.io/distroless/cc

COPY --from=build-env /sentry-kubernetes /
ENTRYPOINT ["/sentry-kubernetes"]
