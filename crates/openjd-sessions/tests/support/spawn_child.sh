#!/bin/sh
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)

# Spawns a child, then emits its OWN output concurrently with the child's.
#
# The parent must NOT block in `wait` before its loop. A group-kill test needs
# both processes to be actively producing output when the kill lands, so that
# "neither loop finished" is evidence the kill reached BOTH. If the parent sat in
# `wait` until the child exited, its loop would never start within the test's
# timeout and any assertion about the parent's output would hold whether or not
# the kill reached it -- passing even if the parent were orphaned and still
# waiting. The trailing `wait` keeps the parent alive until the child is done in
# the no-kill case.
DIR=$(dirname "$0")
"$DIR/long_running.sh" &
CHILD=$!
for i in $(seq 0 19); do
    echo "Log from runner $i"
    sleep 1
done
wait $CHILD
