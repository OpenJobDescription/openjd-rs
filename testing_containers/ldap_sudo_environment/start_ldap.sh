#!/bin/bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# Copyright by contributors to this project.
# SPDX-License-Identifier: (Apache-2.0 OR MIT)

set -e

rm -f /run/slapd/slapd.pid /run/nslcd/nslcd.pid /run/nscd/nscd.pid /var/run/nscd/nscd.pid
mkdir -p /run/nscd
/usr/sbin/slapd -h 'ldap:/// ldapi:///' -u openldap -g openldap
/usr/sbin/nslcd
/usr/sbin/nscd
