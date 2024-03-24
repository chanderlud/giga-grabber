#Dockerfile
FROM rust:latest

# Set the working directory to /app
WORKDIR /app

COPY . ./

# Build the dependencies
RUN cargo build --release

# Set the entrypoint to the binary
ENTRYPOINT ["./target/release/giga_grabber"]

EXPOSE 5000

# Giga Grabber accepts no arguments
CMD []