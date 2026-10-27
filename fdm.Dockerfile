# fdm for the landing-challenge server (docs/challenges.md): a static Linux
# binary, without the sim link. Export it, or copy it into another image:
#   docker build -f fdm.Dockerfile -o out .
#   COPY --from=<this build> /fdm /usr/local/bin/fdm
FROM rust:1.99-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p cli --bin fdm --no-default-features

FROM scratch
COPY --from=build /src/target/release/fdm /fdm
