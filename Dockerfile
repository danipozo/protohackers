# Adapted from https://fasterthanli.me/articles/remote-development-with-rust-on-fly-io
# Build with DOCKER_BUILDKIT=1

################################################################################
# Rust builder image based on ubuntu:20.04
FROM ubuntu:20.04 AS builder

# Install base utils: curl to grab rustup, gcc + build-essential for linking.
# we could probably reduce that a bit but /shrug
RUN set -eux; \
		export DEBIAN_FRONTEND=noninteractive; \
		apt update; \
		apt install --yes --no-install-recommends \
			curl ca-certificates \
			gcc build-essential \
			; \
		apt clean autoclean; \
		apt autoremove --yes; \
		rm -rf /var/lib/{apt,dpkg,cache,log}/; \
		echo "Installed base utils!"

# Install rustup
RUN set -eux; \
		curl --location --fail \
			"https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init" \
			--output rustup-init; \
		chmod +x rustup-init; \
		./rustup-init -y --no-modify-path; \
		rm rustup-init;

# Add rustup to path, check that it works
ENV PATH=${PATH}:/root/.cargo/bin
RUN set -eux; \
		rustup --version;

# Build some code!
# Careful: now we need to cache `/root/.cargo/` rather than `/usr/local/cargo`
# since rustup installed things differently than in the rust build image
ARG BINARY
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/app/target \
		--mount=type=cache,target=/root/.cargo/registry \
		--mount=type=cache,target=/root/.cargo/git \
		--mount=type=cache,target=/root/.rustup \
		set -eux; \
		rustup install stable; \
	 	cargo build --release; \
		objcopy --compress-debug-sections target/release/$BINARY ./$BINARY

################################################################################
FROM ubuntu:20.04

ARG BINARY
WORKDIR app

COPY --from=builder /app/$BINARY .
RUN mv $BINARY binary

CMD ["./binary"]
