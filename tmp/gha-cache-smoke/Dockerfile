# Tiny multi-layer image so BuildKit can show CACHED hits on re-import.
FROM alpine:3.20

RUN apk add --no-cache curl

COPY hello.txt /hello.txt

RUN cat /hello.txt && echo "gha-cache-smoke ok"