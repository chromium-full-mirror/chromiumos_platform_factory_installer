#!/bin/sh
# Copyright 2015 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

# Prepares system environment to start ChromeOS factory installer.

# We want to print the console UI to TTY_FILE, redirect its contents into TTY,
# and print bash trace logs to LOG_FILE (via LOG_TTY if available).
TTY=""
TTY_FILE=/var/log/factory_install.output
LOG_TTY=""
LOG_FILE=/var/log/factory_install.log
export TTY TTY_FILE LOG_TTY LOG_FILE

# Checks if given parameter is a valid TTY (or PTS) device file.
is_valid_tty() {
  [ -c "$1" ] && (echo "" >"$1") 2>/dev/null
}

# Determine the right console by following order:
#  - The last non-empty console= from cmdline.
#  - Frecon if frecon is working fine.
#  - /dev/tty1 if available (for Non-Freon boards).
#  - /dev/null if nothing available.
find_best_tty() {
  local entry="$(cat /proc/cmdline)"
  local ttys=""
  local tty=""

  while echo "${entry}" | grep -q console= ; do
    entry="${entry#*console=}"
    ttys="${entry%%[ ,]*} ${ttys}"
    entry="${entry#* }"
  done

  for tty in ${ttys}; do
    if is_valid_tty "/dev/${tty}"; then
      TTY="${tty}"
      break
    fi
  done

  if [ -z "${TTY}" ]; then
    # TODO(hungte) Check and enable Frecon.
    if is_valid_tty "/dev/tty1"; then
      TTY=/dev/tty1
    else
      TTY=/dev/null
    fi
  fi

  # Enable LOG_TTY if available (currently only if TTY looks like VT1).
  # TODO(hungte) Support other non-VT1 TTYs.
  if is_valid_tty "${TTY%1}3"; then
    LOG_TTY="${TTY%1}3"
  fi
}

main() {
  find_best_tty

  # Setup and display log files
  mkdir -p "$(dirname "${TTY_FILE}")" "$(dirname "${LOG_FILE}")"
  touch "${TTY_FILE}" "${LOG_FILE}"

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
  exec setsid sh -c \
    "exec script -afqc 'factory_install.sh' ${TTY_FILE} <${TTY} >>${TTY} 2>&1"
}

main "$@"
