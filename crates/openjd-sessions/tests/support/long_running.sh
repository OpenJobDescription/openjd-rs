#!/bin/sh
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)
trap 'echo Trapped; exit 1' TERM
for i in $(seq 0 19); do
    echo "Log from test $i"
    sleep 1
done
