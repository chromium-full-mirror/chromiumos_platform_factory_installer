#!/bin/sh
# Copyright 2015 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Prepares system environment to start ChromeOS factory installer.

# We want to print the console UI to TTY_FILE, redirect its contents into TTY,
# and print bash trace logs to LOG_FILE (via LOG_TTY if available).
# Note TTY_CONSOLE may be changed by build script (from make.conf).
TTY_CONSOLE=""
TTY=""
TTY_FILE=/var/log/factory_install.output
LOG_TTY=""
LOG_FILE=/var/log/factory_install.log
OVERLORD_READY=1
export TTY TTY_FILE LOG_TTY LOG_FILE OVERLORD_READY TTY_CONSOLE

. "$(dirname "$0")/ping_shopfloor.sh"

# Checks if given parameter is a valid TTY (or PTS) device file.
is_valid_tty() {
  [ -c "$1" ] && (echo "" >"$1") 2>/dev/null
}

# Extracts tokens from kernel command line.
get_kernel_var() {
  local token="$1="
  local entry="$(cat /proc/cmdline)"
  local result=""

  while echo "${entry}" | grep -q "${token}" ; do
    entry="${entry#*${token}}"
    result="${entry%%[ ,]*} ${result}"
    entry="${entry#* }"
  done
  echo "${result}"
}

# Determine the right console by following order:
#  - The last non-empty console= from cmdline.
#  - TODO(hungte) Frecon if frecon is working fine.
#  - ${TTY_CONSOLE} if none empty (which may be a list).
#  - /dev/tty1 if available (for Non-Freon boards).
#  - /dev/null if nothing available.
find_best_tty() {
  local ttys="$(get_kernel_var console)"
  local tty_name="" tty_path=""

  TTY=""
  LOG_TTY=""

  for tty_name in ${ttys} ${TTY_CONSOLE} tty1 null; do
    tty_path="/dev/${tty_name}"
    if is_valid_tty "${tty_path}"; then
      TTY="${tty_path}"
      break
    fi
  done

  # Devices using tty2 are actually using tty1 as default console.
  [ "${TTY}" = /dev/tty2 ] && TTY=/dev/tty1

  # Enable LOG_TTY if available (currently only if TTY is VT1).
  if [ "${TTY}" = /dev/tty1 ] && is_valid_tty /dev/tty3; then
    LOG_TTY=/dev/tty3
  fi

  # Report to kernel.
  [ -e "/dev/kmsg" ] && (echo "$0: TTY=${TTY}" >>/dev/kmsg)
}


main() {
  find_best_tty

  # Setup and display log files
  mkdir -p "$(dirname "${TTY_FILE}")" "$(dirname "${LOG_FILE}")"
  touch "${TTY_FILE}" "${LOG_FILE}"

  local omahaserver="$(get_kernel_var omahaserver)"
  if [ -n "${omahaserver}" ]; then
    # Should be netboot-ready environment. Try to start Overlord service.
    bringup_network &&
      register_to_overlord "${omahaserver}" "${TTY_FILE}" "${LOG_FILE}" ||
      OVERLORD_READY=
  fi

  if [ -n "${LOG_TTY}" ]; then
    echo "
        ---------------------------------------------------------
        ChromeOS Factory Installer - $(date)

         Press [q] to refresh, [g/G] to begin/end of logs, [b/f]
         to backward/forward pages, or navigation keys to scroll.
        ---------------------------------------------------------" >${LOG_FILE}
    LOG_VIEWER_COMMAND="while true; do secure_less.sh <${LOG_FILE}; done"
    # 'script' here helps to reset control terminal environment on LOG_TTY.
    setsid sh -c \
      "script -afqc '${LOG_VIEWER_COMMAND}' /dev/null <${LOG_TTY} >${LOG_TTY}" &
  fi

  # Use the System-V way to specify controlling TTY (CTTY): create a new session
  # (setsid) and the first opened TTY will be CTTY. 'script' is used (instead of
  # redirection) to keep input and output stream in terminal type.
  # Service may be executed as 'exec' so we have to explicitly sleep here
  # otherwise kernel will panic with 'init aborted' and hard to debug.
  exec setsid sh -c \
    "exec script -afqc 'factory_install.sh || sleep 1d' ${TTY_FILE} \
     <${TTY} >>${TTY} 2>&1"
}

main "$@"
