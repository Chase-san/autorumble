#!/usr/bin/env bash
# Runs RoboRumble clients on this machine, to fill the rumbles of a server running here.
#
#   tools/local_clients.sh setup ROBOCODE_DIR ROBOTS_DIR   make the client folders
#   tools/local_clients.sh start [HOURS]                   run them in the background, 3 hours by default
#   tools/local_clients.sh stop                            stop them
#   tools/local_clients.sh status                          show what is running
#
# ROBOCODE_DIR is a Robocode installation whose client supports HIDENAMES and CLEARDATA. ROBOTS_DIR holds robot
# jars. Each client gets its own folder under $CLIENTS, because a client writes to its config, robot
# database and robot data. Set the number of clients per rumble below. Robot jars are hard links, so
# the folders stay small. A rumble with an empty participant list gets no clients. Jars missing from
# ROBOTS_DIR are downloaded by the client from the URL in the list, like any other client would.
#
# Each iteration runs in a new JVM: on Java 21 Robocode cannot kill a runaway robot thread, so threads
# from a misbehaving robot end with the JVM instead of piling up.
set -uo pipefail

CLIENTS="${CLIENTS:-$HOME/autorumble-clients}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER="${SERVER:-127.0.0.1:8652}"
UPLOADER="${UPLOADER:-$(hostname -s)}"

# Rumble, clients, battles per iteration. Keep the total below the number of CPU cores.
RUMBLES="autorumble 4 20
openrumble 18 20
automelee6 4 5
openmelee6 8 5"

participants() {  # the "name version" of each robot on a rumble's list
  cut -d, -f1 "$REPO/participants/$1.txt" | grep .
}

setup() {
  local robocode="$1" robots="$2"
  mkdir -p "$CLIENTS"
  while read -r rumble count battles; do
    [ "$(participants "$rumble" | wc -l)" -gt 0 ] || { echo "$rumble: no participants, skipping"; continue; }
    for number in $(seq -w 1 "$count"); do
      local client="$CLIENTS/$rumble-$number"
      mkdir -p "$client/robots" "$client/roborumble/temp" "$client/roborumble/files"
      cp -r "$robocode/libs" "$robocode/config" "$robocode/set_java_options.sh" "$client/"
      cp "$robocode/roborumble/roborumble.properties" "$client/roborumble/"
      sed -e "s#localhost:8652#$SERVER#g" -e "s/^USER=.*/USER=$UPLOADER/" -e "s/^ITERATE=.*/ITERATE=NOT/" \
          -e "s/^NUMBATTLES=.*/NUMBATTLES=$battles/" "$REPO/client/$rumble.txt" > "$client/roborumble/$rumble.txt"
      participants "$rumble" | tr ' ' '_' | while read -r name; do
        if [ -e "$robots/$name.jar" ] && [ ! -e "$client/robots/$name.jar" ]; then
          ln "$robots/$name.jar" "$client/robots/"
        fi
      done
    done
    echo "$rumble: $count clients"
  done <<< "$RUMBLES"
}

# One client until the deadline. It leads its own process group, so stop can end it and its JVM.
loop() {
  local client="$1" rumble="$2" deadline="$3"
  cd "$client" || exit 1
  echo $$ > client.pid
  # shellcheck disable=SC1091
  . ./set_java_options.sh > /dev/null
  while [ "$(date +%s)" -lt "$deadline" ]; do
    java -cp "libs/*" -Xmx1024M -XX:ActiveProcessorCount=2 -XX:+UseSerialGC -XX:+IgnoreUnrecognizedVMOptions \
      "--add-opens=java.base/sun.net.www.protocol.jar=ALL-UNNAMED" \
      "--add-opens=java.base/java.lang.reflect=ALL-UNNAMED" \
      -Djava.awt.headless=true roborumble.RoboRumbleAtHome "./roborumble/$rumble.txt" > client.log 2>&1
    sleep 2
  done
  rm -f client.pid
}

running() {  # prints the pid file of each client still running
  local pidfile
  for pidfile in "$CLIENTS"/*/client.pid; do
    [ -e "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null && echo "$pidfile"
  done
}

start() {
  local hours="${1:-3}" deadline client rumble
  deadline=$(( $(date +%s) + hours * 3600 ))
  for client in "$CLIENTS"/*-[0-9]*/; do
    client="${client%/}"
    [ -e "$client/client.pid" ] && kill -0 "$(cat "$client/client.pid")" 2>/dev/null && continue
    rumble="$(basename "$client")"
    rumble="${rumble%-*}"
    (setsid nohup bash "${BASH_SOURCE[0]}" _loop "$client" "$rumble" "$deadline" < /dev/null > /dev/null 2>&1 &)
    sleep 1  # so their battles and uploads do not all line up
  done
  echo "started, running until $(date -d "@$deadline" '+%H:%M')"
}

stop() {
  local pidfile
  for pidfile in $(running); do
    kill -- "-$(cat "$pidfile")" 2>/dev/null
    rm -f "$pidfile"
  done
  echo "stopped"
}

status() {
  echo "$(running | wc -l) clients running, $(pgrep -c -f RoboRumbleAtHome) JVMs, load $(cut -d' ' -f1 /proc/loadavg)"
}

case "${1:-}" in
  setup) setup "$2" "$3" ;;
  start) start "${2:-3}" ;;
  stop) stop ;;
  status) status ;;
  _loop) loop "$2" "$3" "$4" ;;
  *) sed -n '2,7p' "${BASH_SOURCE[0]}"; exit 2 ;;
esac
