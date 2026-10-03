# Image with the static ZHeroDiZk server binaries.
# Build context: a directory holding zhd-rendezvous, zhd-relay and zhd-utils
# for the target architecture, e.g.
#   docker build -f packaging/docker/Dockerfile --build-arg BIN_DIR=bin/amd64 .
FROM alpine:3.20
ARG BIN_DIR
RUN adduser -S -D -H -h /data zherodizk && mkdir -p /data && chown zherodizk /data
COPY ${BIN_DIR}/zhd-rendezvous ${BIN_DIR}/zhd-relay ${BIN_DIR}/zhd-utils /usr/local/bin/
USER zherodizk
WORKDIR /data
VOLUME /data
EXPOSE 21115 21116 21117 21118 21119
CMD ["zhd-rendezvous", "-k", "_"]
