# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

FROM rust:1-bookworm

ENV OPENJD_TEST_SUDO_TARGET_USER=targetuser
ENV OPENJD_TEST_SUDO_SHARED_GROUP=sharedgroup
ENV OPENJD_TEST_SUDO_DISJOINT_USER=disjointuser
ENV OPENJD_TEST_SUDO_DISJOINT_GROUP=disjointgroup

WORKDIR /config

COPY testing_containers/ldap_sudo_environment/changePassword.ldif /config
COPY testing_containers/ldap_sudo_environment/addUsersGroups.ldif /config
COPY testing_containers/ldap_sudo_environment/addUsersToSharedGroup.ldif /config
COPY testing_containers/ldap_sudo_environment/start_ldap.sh /config

# We set up two users for our tests:
#  1) hostuser -- the user that will be running the tests.
#  2) targetuser -- the user that we'll be running subprocesses as in the
#     the cross-account tests.
#  3) disjointuser -- a user used in temporary directory creation tests.
# These accounts belong to the following groups:
#   hostuser: hostuser, sharedgroup
#   targetuser: targetuser, sharedgroup
#   disjointuser: disjointuser, disjointgroup

# Step 1: Install packages and configure slapd via debconf + dpkg-reconfigure.
# Remove -v from debconf-set-selections (exits non-zero on warnings for note/error/boolean types).
# Remove template lines rejected by newer slapd packages.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive && \
    apt-get install -y psmisc vim screen slapd ldap-utils && \
    echo "slapd slapd/password1 password" | debconf-set-selections && \
    echo "slapd slapd/password2 password" | debconf-set-selections && \
    echo "slapd slapd/internal/adminpw password" | debconf-set-selections && \
    echo "slapd slapd/internal/generated_adminpw password" | debconf-set-selections && \
    echo "slapd slapd/no_configuration boolean false" | debconf-set-selections && \
    echo "slapd slapd/dump_database select when needed" | debconf-set-selections && \
    echo "slapd slapd/domain string environment.internal" | debconf-set-selections && \
    echo "slapd slapd/move_old_database boolean true" | debconf-set-selections && \
    echo "slapd slapd/purge_database boolean false" | debconf-set-selections && \
    echo "slapd slapd/dump_database_destdir string /var/backups/slapd-VERSION" | debconf-set-selections && \
    echo "slapd shared/organization string environment.internal" | debconf-set-selections && \
    dpkg-reconfigure slapd && \
    echo "BASE    dc=environment,dc=internal" >> /etc/ldap/ldap.conf && \
    echo "URI     ldap://localhost" >> /etc/ldap/ldap.conf

# Step 2: Start slapd directly, populate LDAP data, then stop it.
RUN rm -f /run/slapd/slapd.pid && \
    /usr/sbin/slapd -h 'ldap:/// ldapi:///' -u openldap -g openldap && \
    ldapmodify -Q -Y EXTERNAL -H ldapi:/// -f /config/changePassword.ldif && \
    ldapadd -x -D cn=admin,dc=environment,dc=internal -w password -f /config/addUsersGroups.ldif && \
    ldapmodify -xcD cn=admin,dc=environment,dc=internal -w password -f /config/addUsersToSharedGroup.ldif && \
    kill $(cat /run/slapd/slapd.pid) && sleep 1

# Step 3: Install nslcd/libnss-ldapd/libpam-ldapd (these don't need slapd running).
RUN export DEBIAN_FRONTEND=noninteractive && \
    echo "nslcd nslcd/ldap-uris string ldap://localhost/" | debconf-set-selections && \
    echo "nslcd nslcd/ldap-base string dc=environment,dc=internal" | debconf-set-selections && \
    echo "libnss-ldapd:amd64 libnss-ldapd/nsswitch multiselect passwd, group, shadow" | debconf-set-selections && \
    apt-get install -y libnss-ldapd libpam-ldapd && \
    echo "session optional        pam_mkhomedir.so skel=/etc/skel umask=007" >> /etc/pam.d/common-session

# Step 4: Configure nslcd to use localhost and ensure nsswitch is correct.
RUN sed -i 's|^uri .*|uri ldap://localhost/|' /etc/nslcd.conf && \
    touch /etc/netgroup

# Step 5: Start slapd + nslcd + nscd, create home dirs, install sudo-ldap.
RUN rm -f /run/slapd/slapd.pid /run/nslcd/nslcd.pid && \
    mkdir -p /run/nscd && \
    /usr/sbin/slapd -h 'ldap:/// ldapi:///' -u openldap -g openldap && \
    /usr/sbin/nslcd && \
    /usr/sbin/nscd && \
    mkhomedir_helper hostuser 007 && \
    mkhomedir_helper targetuser 007 && \
    mkhomedir_helper disjointuser 007 && \
    apt-get install -y sudo-ldap && \
    echo "hostuser ALL=(${OPENJD_TEST_SUDO_TARGET_USER},hostuser) NOPASSWD: ALL" > /etc/sudoers.d/hostuser && \
    rm -rf /var/lib/apt/lists/* && \
    apt-get clean

COPY . /code/
WORKDIR /code

RUN cargo build --tests -p openjd-sessions --features test-utils

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:$PATH

CMD ["/bin/sh", "-c", "/config/start_ldap.sh && chown -R hostuser:hostuser /code && sudo -u hostuser env CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:$PATH OPENJD_TEST_SUDO_TARGET_USER=$OPENJD_TEST_SUDO_TARGET_USER OPENJD_TEST_SUDO_SHARED_GROUP=$OPENJD_TEST_SUDO_SHARED_GROUP OPENJD_TEST_SUDO_DISJOINT_USER=$OPENJD_TEST_SUDO_DISJOINT_USER OPENJD_TEST_SUDO_DISJOINT_GROUP=$OPENJD_TEST_SUDO_DISJOINT_GROUP cargo test -p openjd-sessions --features test-utils -- --include-ignored"]
