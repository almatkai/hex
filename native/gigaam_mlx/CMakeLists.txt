# gigaam_mlx: GigaAM-Multilingual on MLX, built by build.rs through the
# `cmake` crate on aarch64 macOS only. SKELETON, UNTESTED.
# See docs/plans/gigaam-mlx.md ("Build").
cmake_minimum_required(VERSION 3.24)
project(gigaam_mlx LANGUAGES C CXX OBJC OBJCXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(NOT APPLE OR NOT CMAKE_SYSTEM_PROCESSOR MATCHES "arm64|aarch64")
  message(FATAL_ERROR "gigaam_mlx needs Apple Silicon; MLX has no Intel Metal backend")
endif()

# MLX v0.32.4, pinned by commit (tags can move). The ai-babai artifacts were
# converted with mlx 0.32.0; safetensors loading does not depend on the
# exact patch release.
set(MLX_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_BENCHMARKS OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_PYTHON_BINDINGS OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_PYTHON_STUBS OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_GGUF OFF CACHE BOOL "" FORCE)
set(MLX_BUILD_SAFETENSORS ON CACHE BOOL "" FORCE)
set(MLX_BUILD_METAL ON CACHE BOOL "" FORCE)
set(MLX_BUILD_CPU ON CACHE BOOL "" FORCE)   # MLX needs it to link; Hex never selects the CPU device
set(MLX_USE_CCACHE OFF CACHE BOOL "" FORCE)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
# OFF: precompiled mlx.metallib, bundled in Hex.app/Contents/Resources.
# ON: kernels compiled from source at first use (smaller bundle, slower
# first dictation; Hex's prewarm hides it). ASSUMPTION: JIT still needs a
# small metallib; measure both on the first Mac build.
option(GIGAAM_MLX_METAL_JIT "Compile MLX kernels at run time" OFF)
set(MLX_METAL_JIT ${GIGAAM_MLX_METAL_JIT} CACHE BOOL "" FORCE)

include(FetchContent)
FetchContent_Declare(
  mlx
  GIT_REPOSITORY https://github.com/ml-explore/mlx.git
  GIT_TAG 36969c094354086e15d2a327a9c50feb20b9cf11 # v0.32.4
  GIT_SHALLOW FALSE)
FetchContent_MakeAvailable(mlx)

add_library(gigaam_mlx STATIC gigaam_mlx.cpp)
target_include_directories(gigaam_mlx PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(gigaam_mlx PRIVATE mlx)

install(TARGETS gigaam_mlx ARCHIVE DESTINATION lib)
# ASSUMPTION: MLX's static target is `mlx` and its metallib lands in
# ${mlx_BINARY_DIR}/mlx/backend/metal/kernels/mlx.metallib. build.rs links
# libmlx.a from the build tree and copies the metallib into OUT_DIR.
install(FILES ${mlx_BINARY_DIR}/mlx/backend/metal/kernels/mlx.metallib DESTINATION lib OPTIONAL)
