#!/bin/sh

# Copyright 2013 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

sf_log() {
  echo "$(date) get_server_payload_json_path: $*" >&2
}

get_server_payload_json_path() {
  local omaha="$1"
  local eth_interface="$2"

  # Try to get resource map from Umpire.
  local sn="$(vpd -i RO_VPD -g serial_number)" || sn=""
  local mlb_sn="$(vpd -i RO_VPD -g mlb_serial_number)" || mlb_sn=""
  local mac_addr="$(ip link show ${eth_interface} | grep link/ether |
    tr -s ' '| cut -d ' ' -f 3)"
  local resourcemap=""
  local mac="mac.${eth_interface}=${mac_addr};"
  local values="sn=${sn}; mlb_sn=${mlb_sn}; board=${BOARD}; ${mac}"
  local empty_values="firmware=; ec=; stage=;"
  local header="X-Umpire-DUT: ${values} ${empty_values}"
  local target="${omaha}/resourcemap"
  sf_log "${header}"

  if resourcemap="$(wget -O - --header "${header}" "${target}")"; then
    sf_log "got resourcemap: ${resourcemap}"
    local json_name="$(echo "${resourcemap}" | grep "^payloads: " |
      cut -d ' ' -f 2)"
    if [ -n "${json_name}" ]; then
      echo "res/${json_name}"
    else
      sf_log "'payloads' not found in resourcemap, please upgrade Umpire."
    fi
  else
    sf_log "could not fetch resourcemap, fallback to mini-omaha."
    local json_path="static/${BOARD}.json"
    if curl -f "${omaha}/${json_path}" >/dev/null 2>&1; then
      echo "${json_path}"
    else
      sf_log "could not fetch ${json_path}."
    fi
  fi
}

# Bring up network
bringup_network() {
  # Probe USB Ethernet devices.
  local module
  local module_name
  local result=1
  for module in /lib/modules/*/kernel/drivers/net/usb/*; do
    module_name="$(basename "${module%.ko}")"
    modprobe ${module_name}
  done

  # Try to bring up network and get an IP address on each Ethernet device.
  for iface in $(ifconfig -a | grep -Eo 'eth[0-9]+'); do
    ifconfig ${iface} up || true
    udhcpc -t 3 -f -q -n -i ${iface} -s /etc/udhcpc.script && result=0
  done
  return ${result}
}

# Try to register into Overlord server if available.
register_to_overlord() {
  local omaha_url="$1"
  local tty_file="$2"
  local log_file="$3"
  local machine_id=""

  local server="${omaha_url#*://}"
  server="${server%%:*}"
  server="${server%% }"
  server="${server## }"
  [ -n "${server}" ] || return 1

  # Build machine ID by MAC addresses.
  local eth="$(netstat -r | grep '^default ')"
  machine_id="$(ip link show ${eth##* } | grep 'link/ether' |
                sed 's/.*ether //;s/ brd .*//' | tr '\n' ',' | sed 's/,$//')"
  [ -n "${machine_id}" ] || machine_id="UnknownMachineId.$(uuidgen 2>/dev/null)"

  local header_tty="$(mktemp)" header_log="$(mktemp)"
  local header='{"name":"register",
                 "params":{"sid":"%s","mid":"%s","format":%s,"mode":4}}\r\n'
  printf "${header}" "Console" "${machine_id}" "1" >"${header_tty}"
  printf "${header}" "DebugLog" "${machine_id}" "0" >"${header_log}"

  # nc@busybox may have problem in receiving data from pipe (due to buffer size
  # and timeout), so we should write the command into a scripting file and let
  # busybox create the pipe internally.
  local tty_piper="$(mktemp)" log_piper="$(mktemp)"
  echo "tail -n +0 -qF ${header_tty} ${tty_file}" >"${tty_piper}"
  echo "tail -n +0 -qF ${header_log} ${log_file}" >"${log_piper}"
  chmod a+rx "${tty_piper}" "${log_piper}"
  local connect_cmd="while true; do busybox nc ${server} 4455 -e %s;
                     sleep 10; done"

  if [ -n "${tty_file}" ]; then
    setsid sh -c "$(printf "${connect_cmd}" ${tty_piper})" &
  fi
  setsid sh -c "$(printf "${connect_cmd}" ${log_piper})" &
}
