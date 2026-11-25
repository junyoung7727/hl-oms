#!/bin/sh
# crypto-lake-audit.sh <venue>: the daily lake audit of one venue, run by
# crypto-lake-audit@.service under the daily-job lock
# (docs/operations/observability.md §4).
#
# 1. md-lake verify for each of the 9 UTC dates from yesterday back that has
#    a sealed L0 file and no verify result, no verify stamp, a stamp later
#    than now, or a sealed file whose status changed after the stamp.
# 2. The qdp auditor, which reads those results.
#
# Exit: 2 = usage refused (an empty setting, or no qdp checkout); else the
# auditor's status when it is not 0; else 5 when a verify failed; else 0.
# Needs GNU find and date.

venue=$1

need() {
	if [ -z "$2" ]; then
		echo "crypto-lake-audit: $1 is empty" >&2
		exit 2
	fi
}
need venue "$venue"
need LAKE "$LAKE"
need STATE_DIRECTORY "$STATE_DIRECTORY"
need QDP_CHECKOUT "$QDP_CHECKOUT"
need UV "$UV"
need MD_LAKE "$MD_LAKE"

root="$LAKE/crypto/l0/venue=$venue"
now=$(date -u +%s) || exit 2
verify_failed=

# Prints the first sealed file of date $1 (more find arguments in $2...);
# fails when find fails.
sealed() {
	on=$1
	shift
	find "$root" -path "*/date=$on/*.qraw.zst" -type f "$@" -print -quit
}

i=1
while [ "$i" -le 9 ]; do
	d=$(date -u -d "@$((now - i * 86400))" +%Y-%m-%d) || exit 2
	i=$((i + 1))
	[ -d "$root" ] || continue
	if ! first=$(sealed "$d"); then
		echo "crypto-lake-audit: cannot list the sealed files of $venue $d" >&2
		verify_failed=1
		continue
	fi
	[ -n "$first" ] || continue
	result="$LAKE/crypto/l0/_verify/venue=$venue/date=$d.json"
	stamp="$STATE_DIRECTORY/verify-$venue-$d.stamp"
	if [ -e "$result" ] && [ -e "$stamp" ]; then
		# A stamp later than now (the clock was set back after the touch)
		# would hide the files sealed since: such a stamp counts as missing.
		future=$(find "$stamp" -newermt "@$now" -print) || future=1
		if [ -z "$future" ]; then
			# A rename changes a file's ctime, not its mtime: -cnewer compares
			# the ctime with the stamp's mtime (the touch below).
			if ! changed=$(sealed "$d" -cnewer "$stamp"); then
				echo "crypto-lake-audit: cannot list the sealed files of $venue $d" >&2
				verify_failed=1
				continue
			fi
			[ -n "$changed" ] || continue
		fi
	fi
	# The stamp is taken before verify lists the files: a file sealed while
	# verify runs is newer than the stamp, so the next run verifies again.
	if ! touch "$stamp.new"; then
		echo "crypto-lake-audit: cannot write $stamp.new" >&2
		verify_failed=1
		continue
	fi
	sleep 1
	if "$MD_LAKE" verify --venue "$venue" --lake "$LAKE" --date "$d" --json >/dev/null; then
		# mv keeps the mtime of the touch.
		if ! mv -f "$stamp.new" "$stamp"; then
			echo "crypto-lake-audit: cannot write $stamp" >&2
			verify_failed=1
		fi
	else
		echo "crypto-lake-audit: md-lake verify failed for $venue $d; the old stamp stays" >&2
		rm -f "$stamp.new"
		verify_failed=1
	fi
done

if ! cd "$QDP_CHECKOUT"; then
	echo "crypto-lake-audit: no qdp checkout at $QDP_CHECKOUT" >&2
	exit 2
fi
"$UV" run --no-sync python -m quant_data.crypto audit --lake "$LAKE" --venue "$venue" --state "$STATE_DIRECTORY/audit-$venue.json" --vm-url http://127.0.0.1:8428
status=$?
if [ "$status" -ne 0 ]; then
	exit "$status"
fi
if [ -n "$verify_failed" ]; then
	exit 5
fi
exit 0
