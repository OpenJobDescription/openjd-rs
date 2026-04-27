#!/bin/bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)

set -eux

# Run this from the root of the repository
if ! test -f Cargo.toml
then
    echo "Must run from the root of the repository"
    exit 1
fi

USE_LDAP="False"
BUILD_ONLY="False"
while [[ "${1:-}" != "" ]]; do
    case $1 in
        -h|--help)
            echo "Usage: run_cross_user_tests.sh [--ldap] [--build-only]"
            exit 1
            ;;
        --ldap)
            echo "Using the LDAP client container image for testing."
            USE_LDAP="True"
            ;;
        --build-only)
            BUILD_ONLY="True"
            ;;
        *)
            echo "Unrecognized parameter: $1"
            exit 1
            ;;
    esac
    shift
done

if test "${USE_LDAP}" == "True"; then
    CONTAINER_HOSTNAME=ldap.environment.internal
    CONTAINER_IMAGE_TAG="openjd_rs_ldap_test"
    CONTAINER_IMAGE_DIR="ldap_sudo_environment"
else
    CONTAINER_HOSTNAME=localuser.environment.internal
    CONTAINER_IMAGE_TAG="openjd_rs_localuser_test"
    CONTAINER_IMAGE_DIR="localuser_sudo_environment"
fi

ARGS="-h ${CONTAINER_HOSTNAME}"

docker build -t "${CONTAINER_IMAGE_TAG}" --build-arg "BUILDKIT_SANDBOX_HOSTNAME=${CONTAINER_HOSTNAME}" --ulimit nofile=1024 --file "testing_containers/${CONTAINER_IMAGE_DIR}/Dockerfile" .

if test "${BUILD_ONLY}" == "True"; then
    exit 0
fi

docker run --name test_openjd_rs_sudo --rm ${ARGS} "${CONTAINER_IMAGE_TAG}:latest"

