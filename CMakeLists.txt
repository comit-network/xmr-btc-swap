cmake_minimum_required(VERSION 3.19)
project(xmr_btc_swap_comit C)

set(CMAKE_CXX_STANDARD 11)

set(BOOST_ROOT "/usr/local/Cellar/boost/1.75.0_2")
include_directories(/usr/local/Cellar/boost/1.75.0_2/include)
include_directories(monero-adaptor/depend/hash)

add_library(xmr_btc_swap_comit
        monero-adaptor/depend/hash/hash.c
        monero-adaptor/depend/hash/include/int-util.h
        monero-adaptor/depend/hash/crypto-ops.c
        monero-adaptor/depend/hash/include/crypto-ops.h
        monero-adaptor/depend/hash/include/keccak.h
        monero-adaptor/depend/hash/include/hash-ops.h
        monero-adaptor/depend/hash/keccak.c
        )
