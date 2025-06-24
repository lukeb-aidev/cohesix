# CLASSIFICATION: COMMUNITY
# Filename: .bashrc v0.1
# Author: Lukas Bower
# Date Modified: 2026-08-09

export CUDA_HOME=/usr
export CUDA_INCLUDE_DIR=$CUDA_HOME/include
export CUDA_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu
export PATH=$CUDA_HOME/bin:$PATH
export LD_LIBRARY_PATH=$CUDA_LIBRARY_PATH:$LD_LIBRARY_PATH
