cmake_minimum_required(VERSION 3.19)
project(xmr_btc_swap_comit C)

set(CMAKE_C_STANDARD 11)

include_directories(monero-adaptor/depend/hash)

add_library(xmr_btc_swap_comit
        monero-adaptor/depend/hash/hash.c
        monero-adaptor/depend/hash/hash.h
        monero-adaptor/depend/hash/int-util.h)
