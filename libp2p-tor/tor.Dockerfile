# set alpine as the base image of the Dockerfile
FROM alpine:latest

# update the package repository and install Tor
RUN apk update && apk add tor
# Set `tor` as the default user during the container runtime
USER tor

EXPOSE 9050
EXPOSE 9051

# Set `tor` as the entrypoint for the image
ENTRYPOINT ["tor"]
