brew update

# See action.yml
brew install cmake boost openssl zmq libpgm miniupnpc expat libunwind-headers git

# We need to build from source to be able to statically link the dependencies
brew reinstall --build-from-source unbound expat 

# We need an older version of protobuf to be able to statically link it
brew install protobuf@21