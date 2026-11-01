#!/usr/bin/env bash
# Checks every listed robot for trouble with hidden names. Each one fights fixed opponents with names
# hidden and shown, and tools/name_check_report.py then shows what differs, and any errors it printed.
#
#   tools/name_check.sh ROBOCODE_DIR ROBOTS_DIR WORKERS [JOBS_FILE]
#
# ROBOCODE_DIR is a Robocode installation, and ROBOTS_DIR holds robot jars. WORKERS is how many battles
# run at once, which should stay below the number of CPU cores. The results go to $WORK/results.tsv.
#
# By default it checks all the robots on the participant lists. A JOBS_FILE checks chosen ones instead,
# with a line for each: rounds|width|height|robot|opponents
set -uo pipefail
R="$1"
ROBOTS="$2"
WORKERS="$3"
JOBS="${4:-}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
N="${WORK:-$HOME/name-check}"

names() { cut -d, -f1 "$REPO/participants/$1.txt" | grep . ; }

DUEL_OPPONENTS=("sample.Walls 1.0" "wiki.BasicGFSurfer 1.02")
MELEE_OPPONENTS="sample.Crazy 1.0,sample.Fire 1.0,sample.Tracker 1.0,sample.SpinBot 1.0,sample.TrackFire 1.0"
SPARE_OPPONENTS="sample.Walls 1.0,sample.Fire 1.0,sample.Tracker 1.0,sample.SpinBot 1.0,sample.TrackFire 1.0"

mkdir -p "$N/classes"
javac -nowarn -cp "$R/libs/*" -d "$N/classes" "$REPO/tools/NameCheck.java" 2>&1 | grep -v '^Note'

if [ -z "$JOBS" ]; then
  JOBS="$N/jobs.txt"
  : > "$JOBS"
  { names openrumble; names autorumble; } | sort -u | while read -r robot; do
    for opponent in "${DUEL_OPPONENTS[@]}"; do
      [ "$robot" = "$opponent" ] || printf '20|800|600|%s|%s\n' "$robot" "$opponent" >> "$JOBS"
    done
  done
  names openmelee6 | sort -u | while read -r robot; do
    case ",$MELEE_OPPONENTS," in
      *",$robot,"*) opponents="$SPARE_OPPONENTS" ;;
      *) opponents="$MELEE_OPPONENTS" ;;
    esac
    case ",$opponents," in *",$robot,"*) continue ;; esac
    printf '10|800|800|%s|%s\n' "$robot" "$opponents" >> "$JOBS"
  done
fi
echo "$(wc -l < "$JOBS") match-ups on $WORKERS workers"

# Each worker has its own folder, because robots save data there. The jars are hard links.
for worker in $(seq 0 $((WORKERS - 1))); do
  home="$N/w$worker"
  rm -rf "$home"
  mkdir -p "$home/robots"
  cp -r "$R/config" "$home/"
  { names openrumble; names openmelee6; names autorumble
    printf '%s\n' "${DUEL_OPPONENTS[@]}"; tr ',' '\n' <<< "$SPARE_OPPONENTS,$MELEE_OPPONENTS"
  } | sort -u | tr ' ' '_' | while read -r name; do
    for directory in "$ROBOTS" "$REPO/robots"; do
      if [ -e "$directory/$name.jar" ]; then ln -f "$directory/$name.jar" "$home/robots/"; break; fi
    done
  done
done

: > "$N/results.tsv"
export R N
# A new JVM for each match-up, so a thread left running by one robot cannot affect the next.
# shellcheck disable=SC2016
shuf "$JOBS" | tr '\n' '\0' | xargs -0 -n 1 -P "$WORKERS" --process-slot-var=SLOT bash -c '
  IFS="|" read -r rounds width height robot opponents <<< "$1"
  cd "$N/w$SLOT" || exit 0
  . "$R/set_java_options.sh" > /dev/null
  timeout 1800 java -cp "$R/libs/*:$N/classes" -Xmx1024M -XX:ActiveProcessorCount=2 -XX:+UseSerialGC \
    -XX:+IgnoreUnrecognizedVMOptions "--add-opens=java.base/sun.net.www.protocol.jar=ALL-UNNAMED" \
    "--add-opens=java.base/java.lang.reflect=ALL-UNNAMED" -Djava.awt.headless=true \
    NameCheck . "$rounds" "$width" "$height" "$robot" "$opponents" "$N/results.tsv" > /dev/null 2>&1
  status=$?
  [ "$status" -eq 0 ] || printf "%s\tfailed\t%s\t\t\t1\texit %s\n" "$robot" "$opponents" "$status" >> "$N/results.tsv"
' _
echo "done: $N/results.tsv"
