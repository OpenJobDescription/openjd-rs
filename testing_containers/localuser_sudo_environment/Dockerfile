# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

FROM rust:1-bookworm

ENV OPENJD_TEST_SUDO_TARGET_USER=targetuser
ENV OPENJD_TEST_SUDO_SHARED_GROUP=sharedgroup
ENV OPENJD_TEST_SUDO_DISJOINT_USER=disjointuser
ENV OPENJD_TEST_SUDO_DISJOINT_GROUP=disjointgroup

RUN apt-get update && apt-get install -y psmisc sudo && \
    rm -rf /var/lib/apt/lists/* && \
    apt-get clean && \
    addgroup sharedgroup && \
    useradd -ms /bin/bash -G sharedgroup targetuser && \
    useradd -ms /bin/bash -G sharedgroup hostuser && \
    echo "hostuser ALL=(targetuser,hostuser) NOPASSWD: ALL" > /etc/sudoers.d/hostuser && \
    addgroup disjointgroup && \
    useradd -ms /bin/bash -G disjointgroup disjointuser

USER hostuser
COPY --chown=hostuser:hostuser . /code/
WORKDIR /code

RUN cargo build --tests -p openjd-sessions --features test-utils

CMD ["cargo", "test", "-p", "openjd-sessions", "--features", "test-utils", "--", "--include-ignored"]
