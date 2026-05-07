#!/bin/sh
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
DIR=$(dirname "$0")
"$DIR/long_running.sh" &
CHILD=$!
wait $CHILD
for i in $(seq 0 19); do
    echo "Log from runner $i"
    sleep 1
done
