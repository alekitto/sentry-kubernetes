FROM public.ecr.aws/docker/library/rust:1.68.0-bullseye as build-env

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
WORKDIR /app
COPY . /app

RUN cargo build --release

FROM gcr.io/distroless/cc

COPY --from=build-env /app/target/release/sentry-kubernetes /
CMD ["/sentry-kubernetes"]
