# Image with the static control server binary.
#   docker build -f packaging/control/Dockerfile --build-arg BIN=bin/zherodizk-control .
FROM alpine:3.20
ARG BIN
RUN adduser -S -D -H -h /nonexistent zherodizk
COPY ${BIN} /usr/local/bin/zherodizk-control
USER zherodizk
EXPOSE 21114
ENV ZHD_LISTEN=0.0.0.0:21114
CMD ["zherodizk-control"]
