// Media server text (feature 022): no DOM, so it is tested with node.

/** The status line's text: state, addresses, streams and players, and the Put.io state */
export function statusText(s) {
  const parts = [];
  if (s.addresses && s.addresses.length) parts.push(s.addresses.join(", "));
  if (s.state === "running") {
    parts.push(`${s.streams_active} playing`);
    parts.push(`${s.players_last_hour} player${s.players_last_hour === 1 ? "" : "s"} in the last hour`);
  }
  if (s.putio && s.putio.state === "ok") parts.push("Put.io: ok");
  if (s.putio && s.putio.state === "error") parts.push(`Put.io: ${s.putio.reason || "not answering"}`);
  return parts.join(" · ");
}

/** The roots that are checked, in the order of `available`, plus those no longer available */
export function pickRoots(available, checked) {
  const known = new Set(available.map((r) => r.key));
  return [...available.map((r) => r.key).filter((k) => checked.has(k)), ...[...checked].filter((k) => !known.has(k))];
}
