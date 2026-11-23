"""Run the native rendered client against an isolated real UDP server.

The client drives ordinary ClientInput messages; there are no server teleports,
damage overrides, accelerated raid clocks or test-only rewards.
Evidence is saved under target/playtest (or --output).
"""
import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import re
import struct
import statistics
import subprocess
import sys
import time

if __package__:
    from . import crouched_lean, inventory_frame_windows, strafe_reversal
else:
    import crouched_lean
    import inventory_frame_windows
    import strafe_reversal


POV_CAPTURES = {
    "recovery-steps": (),
    "ads": ("rest", "ads", "return"),
    "inspect": ("rest", "inspect", "return"),
    "stop-lean": ("rest", "walk", "run", "stop", "crouch", "left", "right", "return"),
    "armed-stride": ("rest", "walk", "run", "turn", "moving-ads", "return"),
    "strafe-reversal": (),
    "west-step": (),
    "prone": ("rest", "prone", "refused", "clear", "return"),
    "stumble": ("rest", "stumble", "contact", "return"),
    "action-fault": ("rest", "fault", "recovering", "interrupted", "persisted", "return"),
    "side-cover": ("rest", "walk", "run", "stop", "crouch", "left", "right", "return"),
    "handling-route": ("rest", "walk", "run", "stop", "crouch", "left", "right",
                       "neutral", "ads", "moving-ads", "hip", "inspect", "cancel", "return"),
}
DUAL_POV = {
    "recovery-steps": (),
    "stop-lean": ("run", "left", "right", "return"),
    "armed-stride": ("turn", "moving-ads", "return"),
    "strafe-reversal": (),
    "west-step": (),
    "prone": ("prone", "refused", "return"),
    "stumble": ("stumble", "contact", "return"),
    "action-fault": POV_CAPTURES["action-fault"],
}
TRACE_ONLY_POV = {"strafe-reversal", "west-step", "recovery-steps"}
DESTRUCTION_CAPTURES = ("intact", "impact", "degraded", "fragment", "settled")


def _gait_displacement(first, last, prefix):
    keys = [f"{prefix}_{axis}" for axis in "xyz"]
    return math.dist([first[key] for key in keys], [last[key] for key in keys])


def gait_metrics(path):
    with path.open(newline="", encoding="utf-8") as stream:
        rows = [{key: float(value) for key, value in row.items()}
                for row in csv.DictReader(stream)]
    assert len(rows) >= 30, f"Insufficient observer telemetry: {path}"
    assert all(math.isfinite(value) for row in rows for value in row.values()), path
    assert all(a["seconds"] < b["seconds"] and a["snapshot"] <= b["snapshot"]
               for a, b in zip(rows, rows[1:])), f"Unordered observer telemetry: {path}"
    result = {}
    for name, low, high in (("walk", 2.5, 3.7), ("run", 4.5, 7.0)):
        samples = []
        transitions = {"loading": [], "steady": [], "releasing": []}
        support = {"left": None, "right": None}
        peak_excursion = None
        for a, b in zip(rows, rows[1:]):
            dt = b["seconds"] - a["seconds"]
            eligible = dt < 0.05 and low < a["speed"] < high and low < b["speed"] < high
            for side in ("left", "right"):
                if not eligible or min(a[f"{side}_weight"], b[f"{side}_weight"]) <= 0.95:
                    support[side] = None
                    continue

                distance = _gait_displacement(a, b, side)
                samples.append((distance, dt))
                delta_weight = b[f"{side}_weight"] - a[f"{side}_weight"]
                phase = ("loading" if delta_weight > 0.005
                         else "releasing" if delta_weight < -0.005 else "steady")
                transitions[phase].append(distance)

                # Per-frame maxima miss small deltas accumulating across a
                # support interval. Break at gaps, speed changes and unloading;
                # compare all three sources over the same maximum excursion.
                first, pairs = support[side] or (a, 0)
                pairs += 1
                support[side] = (first, pairs)
                if pairs < 2:
                    continue
                foot = _gait_displacement(first, b, side)
                if peak_excursion is None or foot > peak_excursion["foot_m"]:
                    peak_excursion = {
                        "side": side, "pairs": pairs,
                        "start_seconds": first["seconds"], "end_seconds": b["seconds"],
                        "start_snapshot": int(first["snapshot"]), "end_snapshot": int(b["snapshot"]),
                        "foot_m": foot,
                        "presented_anchor_m": _gait_displacement(first, b, f"{side}_anchor"),
                        "confirmed_anchor_m": _gait_displacement(first, b, f"{side}_confirmed"),
                    }
        assert samples, f"No consecutive loaded {name} samples: {path}"
        peak, interval = max(samples)
        result[name] = {
            "loaded_pairs": len(samples),
            "max_displacement_m": peak,
            "max_displacement_interval_s": interval,
            "median_displacement_m": statistics.median(distance for distance, _ in samples),
            "peak_support_excursion": peak_excursion,
            "by_weight_transition": {
                phase: {"pairs": len(distances), "max_displacement_m": max(distances, default=None)}
                for phase, distances in transitions.items()
            },
        }
    return result


def process_memory(status):
    fields = dict(re.findall(r"^(VmRSS|VmHWM):\s+(\d+) kB$", status, re.MULTILINE))
    if len(fields) != 2:
        return None  # The process can exit between poll and read.
    return {"rss_kib": int(fields["VmRSS"]), "hwm_kib": int(fields["VmHWM"])}


def sample_solo_memory(client, server):
    samples = []
    start = time.monotonic()
    while client.poll() is None:
        elapsed = time.monotonic() - start
        if elapsed >= 300:
            raise subprocess.TimeoutExpired(client.args, 300)
        row = {"seconds": elapsed}
        for name, process in (("client", client), ("server", server)):
            try:
                row[name] = process_memory(Path(f"/proc/{process.pid}/status").read_text())
            except FileNotFoundError:
                row[name] = None
        samples.append(row)
        try:
            client.wait(timeout=min(1.0, 300 - elapsed))
        except subprocess.TimeoutExpired:
            pass
    return samples


def inventory_payload_totals(server_log):
    records = re.findall(
        r"TK INVENTORY PAYLOAD request_bytes=(\d+) request_messages=(\d+)"
        r" update_bytes=(\d+) update_messages=(\d+)", server_log,
    )
    assert records, "Missing server inventory payload telemetry"
    totals = dict(zip(
        ("request_bytes", "request_messages", "update_bytes", "update_messages"),
        map(int, records[-1]),
    ))
    assert all(totals.values()), "Empty inventory payload direction"
    return totals


def parse_viewport(value):
    match = re.fullmatch(r"([0-9]{1,4})x([0-9]{1,4})", value)
    if match:
        width, height = map(int, match.groups())
        if 600 <= width <= 3840 and 400 <= height <= 2160:
            return width, height
    raise argparse.ArgumentTypeError("expected WIDTHxHEIGHT between 600x400 and 3840x2160")


def png_dimensions(path):
    if path.is_file():
        with path.open("rb") as stream:
            header = stream.read(24)
        if len(header) == 24 and header[:16] == b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR":
            return struct.unpack(">II", header[16:24])
    return None


def loot_contest_evidence(client_logs, server_log):
    key = r"raid=(\d+) item=(\d+) player=(\d+)"

    def rows(text, action, suffix=""):
        return [(match.start(), tuple(map(int, match.groups()))) for match in re.finditer(
            rf"LOOT CONTEST {action} {key}{suffix}\r?$", text, re.MULTILINE)]

    attempts = []
    views = {}
    for text in client_logs:
        press = rows(text, "pressed")
        view = rows(text, "converged", r" owned=(\d+) ground=0 model=0")
        if (len(press) != 1 or len(view) != 1 or view[0][1][:3] != press[0][1]
                or press[0][0] >= view[0][0]):
            raise ValueError("Each client must press once and then observe the same item gone")
        attempts.append(press[0][1])
        _, (_, _, player, owned) = view[0]
        views[player] = owned
    if len(attempts) != 2 or len(views) != 2 or attempts[0][:2] != attempts[1][:2]:
        raise ValueError("Two distinct players must attempt the same raid and item")
    received = rows(server_log, "received")
    if sorted(row for _, row in received) != sorted(attempts):
        raise ValueError("Server must observe both ordinary pickup inputs exactly once")
    commits = rows(server_log, "committed")
    if (len(commits) != 1 or not any(
            row == commits[0][1] and offset < commits[0][0] for offset, row in received)):
        raise ValueError("Expected exactly one server pickup commit after the winner's input")
    _, (raid, item, winner) = commits[0]
    loser = next(player for player in views if player != winner)
    if views[winner] <= 0 or views[loser] != 0:
        raise ValueError("Private inventories must agree with the single committed owner")
    return {"raid": raid, "item": item, "attempts": [row[2] for row in attempts],
            "winner": winner, "owned_quantities": views, "ground_removed_in_both_views": True}


def partial_merge_evidence(log):
    pattern = r"GRID PARTIAL MERGE {} source=(\d+) target=(\d+) revision=(\d+) {}$"
    preview = list(re.finditer(pattern.format("preview", "source_quantity=2 target_quantity=2 remainder=1 total=3"),
                               log, re.MULTILINE))
    confirmed = list(re.finditer(pattern.format("confirmed", "source_quantity=1 target_quantity=3 op=1"),
                                 log, re.MULTILINE))
    if len(preview) != 1 or len(confirmed) != 1 or preview[0].start() >= confirmed[0].start():
        raise ValueError("Expected one preview followed by one confirmed partial merge")
    source, target, revision = map(int, preview[0].groups())
    if source == target or revision != 2 or tuple(map(int, confirmed[0].groups())) != (source, target, revision + 1):
        raise ValueError("Partial merge did not confirm the same stacks at the next revision")
    return {"source": source, "target": target, "preview_revision": revision,
            "confirmed_revision": revision + 1, "before": [2, 2], "after": [1, 3],
            "operation": 1, "input": "synthetic Bevy keyboard", "pending": "not measured"}


def grid_close_ack_evidence(log):
    rows = re.findall(
        r"GRID CLOSE ACK (\w+) raid=(\d+) session=(\d+) op=(\d+) item=(\d+) revision=(\d+) frame=(\d+)$",
        log, re.MULTILINE,
    )
    required = {"submitted", "pending_presented", "tab_close", "ack_applied", "delta_applied", "reopened"}
    if (len(rows) != 7 or {row[0] for row in rows[:-1]} != required
            or any(row[1:6] != rows[0][1:6] for row in rows)):
        raise ValueError("Expected one complete close/ACK trace for the same operation")
    stages = [row[0] for row in rows]
    frames = [int(row[6]) for row in rows]
    submit, pending, close, ack, delta, reopened = map(
        stages.index, ("submitted", "pending_presented", "tab_close", "ack_applied", "delta_applied", "reopened"))
    if (frames != sorted(frames) or frames[close] != frames[pending] + 1
            or not (submit < pending < min(close, ack, delta) and max(close, ack, delta) < reopened)
            or frames[reopened] <= max(frames[i] for i in (close, ack, delta))):
        raise ValueError("Invalid close/ACK frame or event order")
    status = ("ACK_FIRST_NOT_OBSERVED" if ack < close else
              "DELTA_FIRST_NOT_OBSERVED" if delta < close else "CLOSE_BEFORE_ACK")
    if stages[-1] != status:
        raise ValueError("Close/ACK outcome disagrees with trace ordering")
    return dict(zip(("raid", "session", "operation", "item", "starting_revision"), map(int, rows[0][1:6]))) | {
        "status": status, "frames": dict(zip(stages, frames)),
    }


def grid_review_evidence(directory, client_log, viewport, scale, pointer_checkpoint=False, inspect=False):
    states = ("idle", "drag-valid", "drag-invalid", "pending", "hover", "context", "split-quantity")
    observed = {}
    findings = []
    for line in client_log.splitlines():
        row = re.search(
            r"GRID REVIEW (\S+) (\d+)x(\d+) ui_scale=(\S+) revision=(\d+)"
            r" items=(\d+) layout=(inside|CLIPPED)(?: |$)", line,
        )
        if row is None:
            continue
        state, width, height, actual_scale, revision, items, layout = row.groups()
        if state not in states or state in observed:
            findings.append(f"Unexpected or repeated review state: {line}")
            continue
        size = (int(width), int(height))
        path = directory / f"grid-{state}.png"
        png_size = png_dimensions(path)
        observed[state] = {
            "viewport": size, "ui_scale": float(actual_scale), "revision": int(revision),
            "items": int(items), "layout": layout, "png_size": png_size,
            "png": path.name if png_size else None, "layout_log": line,
        }
        if state == "split-quantity":
            split = re.search(
                r" split_item=(\d+) source_quantity=(\d+) quantity=(\d+) min=(\d+) max=(\d+)(?: |$)", line,
            )
            if split is None:
                findings.append("split-quantity: missing displayed quantity or bounds")
            else:
                item, source, quantity, minimum, maximum = map(int, split.groups())
                observed[state]["split"] = {
                    "item": item, "source_quantity": source, "quantity": quantity,
                    "min": minimum, "max": maximum,
                }
                if not (item > 0 and minimum == 1 and minimum <= quantity <= maximum == source - 1):
                    findings.append("split-quantity: displayed quantity or bounds disagree with confirmed stack")
        if (size != viewport or png_size != viewport or layout != "inside"
                or not math.isclose(float(actual_scale), scale, abs_tol=1e-6) or int(items) == 0):
            findings.append(f"{state}: expected {viewport} at scale {scale}; "
                            f"log={size} scale={actual_scale}, PNG={png_size}, layout={layout}")
    missing = [state for state in states if state not in observed]
    required = ("idle", "drag-valid")
    if pointer_checkpoint or inspect:
        required += ("hover", "context")
        order = list(observed)
        if all(state in observed for state in ("idle", "hover", "context", "drag-valid")) and (
            [state for state in order if state in required] != ["idle", "hover", "context", "drag-valid"]
        ):
            findings.append("Pointer review must observe idle, hover, and context before keyboard drag")
        marker = ("GRID INSPECT closed via Escape" if inspect else "GRID POINTER context closed via Escape")
        closed = client_log.find(marker + "; resuming keyboard route")
        context = client_log.find("GRID REVIEW context ")
        drag = re.search(r"GRID REVIEW drag-(?:valid|invalid) ", client_log)
        if context < 0 or closed <= context or drag is None or closed >= drag.start():
            findings.append("Missing ordered context → Escape → keyboard drag evidence")
    findings.extend(f"Required state not observed: {state}" for state in required if state in missing)
    evidence = {"observed": observed, "not_observed": missing, "findings": findings}
    if inspect:
        frames = list(re.finditer(r"GRID INSPECT (open|orbit|zoom|reset) item=(\d+) revision=(\d+) asset=(\S+)", client_log))
        target = re.search(r"GRID POINTER hover/context observed for item=(\d+) revision=(\d+)", client_log)
        if ([frame[1] for frame in frames] != ["open", "orbit", "zoom", "reset"]
                or target is None or not frames
                or not (context < target.start() < frames[0].start() < frames[-1].start() < closed)
                or any(frame.group(2, 3) != target.groups() or frame[4] != frames[0][4] for frame in frames)
                or ("context" in observed and int(target[2]) != observed["context"]["revision"])):
            findings.append("Missing ordered inspection evidence for the same context target")
        evidence["inspection"] = []
        for frame in frames:
            path = directory / f"inspect-{frame[1]}.png"
            size = png_dimensions(path)
            if size != viewport:
                findings.append(f"{path.name}: expected {viewport}, got {size}")
            evidence["inspection"].append({
                "stage": frame[1], "item": int(frame[2]), "revision": int(frame[3]),
                "asset": frame[4], "png": path.name if size else None, "png_size": size,
            })
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--clients", type=int, choices=(1, 2), default=1)
    parser.add_argument("--server-path", type=Path)
    parser.add_argument("--client-path", type=Path)
    parser.add_argument("--video", action="store_true", help="Record native framebuffers and encode timestamped MP4 reviews")
    parser.add_argument("--arena-solo", action="store_true",
                        help="Verify both solo Arena respawns and a clean second session over UDP")
    parser.add_argument("--arena-then-raid", action="store_true",
                        help="After Arena return, check solo raid no-respawn or PvP-to-raid continuity")
    parser.add_argument("--arena-pvp", action="store_true",
                        help="Verify both PvP respawns and a disconnect with two rendered clients")
    parser.add_argument("--voxel-loot", action="store_true", help="Exercise the six voxel loot seeds instead of collecting legacy crates")
    parser.add_argument("--loot-inspect", action="store_true", help="With --voxel-loot, capture three static in-hand views of each confirmed item")
    parser.add_argument("--pet-probe", choices=("unlit-blend", "lit-opaque"),
                        help="Diagnose only the held bottle's translucent surfaces")
    parser.add_argument("--loot-contest", action="store_true",
                        help="Stop after two clients contest one loose item via normal pickup input")
    parser.add_argument("--loot-contest-peer-drop-every", type=int, choices=range(2, 61), metavar="N",
                        help="With --loot-contest, have the second client discard every Nth snapshot")
    parser.add_argument("--inventory-grid", action="store_true",
                        help="Run the confirmed grid checkpoint during solo voxel loot")
    parser.add_argument("--inventory-grid-bench", action="store_true",
                        help="Measure paired idle panel states at the confirmed solo loot checkpoint")
    parser.add_argument("--inventory-grid-review-scale", type=float, metavar="SCALE",
                        help="Enable static grid-only captures at UI scale SCALE (0.75–2.0)")
    parser.add_argument("--inventory-grid-pointer-checkpoint", action="store_true",
                        help="Capture hover and context before keyboard grid operations")
    parser.add_argument("--inventory-grid-inspect", action="store_true",
                        help="Inspect from the context menu before keyboard grid operations")
    parser.add_argument("--inventory-grid-close-ack", action="store_true",
                        help="Diagnose Tab close versus the original server reply during one solo grid move")
    parser.add_argument("--inventory-grid-split", type=int, choices=(1, 2),
                        help="Verify this split quantity against an opt-in three-bandage seed")
    parser.add_argument("--inventory-grid-partial-merge", action="store_true",
                        help="Verify a partial merge of two same-lineage stacks against a private QA server")
    parser.add_argument("--inventory-grid-pickup-hint", action="store_true",
                        help="Check the live ground-item HUD before and after a confirmed backpack Move")
    parser.add_argument("--inventory-telemetry", action="store_true",
                        help="Measure application payloads and Linux process RSS during the solo grid route")
    parser.add_argument("--animation-review", action="store_true", help="Add reciprocal continuous weapon review before the ordinary dual raid")
    parser.add_argument("--combat-hud-checkpoint", action="store_true",
                        help="Capture two server-confirmed combat HUD states")
    parser.add_argument("--weapon-kit", choices=("compact", "suppressed", "afg"),
                        help="Seed server-owned M4A1 kit and physical magazines for native acceptance")
    parser.add_argument("--configure-weapons", action="store_true",
                        help="Choose compact/suppressed kits through real lobby panel actions")
    parser.add_argument("--bench-draft", action="store_true",
                        help="Capture the workbench's confirmed, draft and discarded views before Apply")
    parser.add_argument("--weapon-debug", action="store_true",
                        help="Show live socket/pivot overlays and use a close side observer during animation review")
    parser.add_argument("--clean-art", action="store_true",
                        help="Keep the asset review cameras but hide socket/pivot overlays")
    parser.add_argument("--neutral-art", action="store_true",
                        help="Use neutral white lighting without cast shadows for material review")
    parser.add_argument("--weapon-lod", choices=("auto", "near", "far"), default="auto",
                        help="Explicit LOD inspection override (requires --weapon-debug)")
    parser.add_argument("--pov", choices=POV_CAPTURES, help="Run only the named POV bench after deploy")
    parser.add_argument("--destruction", choices=("wood", "concrete"),
                        help="Review one material through confirmed normal shots and named screenshots")
    parser.add_argument("--viewport", type=parse_viewport,
                        help="Require this exact POV or clean-art framebuffer (WIDTHxHEIGHT)")
    parser.add_argument("--pov-hz", type=int, choices=range(15, 241), metavar="HZ",
                        help="Cap the stop-lean or armed-stride update rate and write the actual camera trace")
    parser.add_argument("--pov-drop-every", type=int, choices=range(2, 61), metavar="N",
                        help="Discard every Nth snapshot on the stop-lean or armed-stride POV leader")
    parser.add_argument("--pov-peer-lossless", action="store_true",
                        help="Disable injected snapshot loss for the dual armed-stride observer")
    parser.add_argument("--no-capture", action="store_true",
                        help="Skip ordinary raid captures (grid-only review is a separate opt-in)")
    parser.add_argument("--audio", action="store_true",
                        help="Enable game audio and verify local and spatial playback sinks")
    args = parser.parse_args()
    if args.destruction and (args.clients != 1 or any((
        args.arena_solo, args.arena_pvp, args.arena_then_raid, args.pov,
        args.pov_hz, args.pov_drop_every, args.pov_peer_lossless,
        args.voxel_loot, args.loot_inspect, args.pet_probe,
        args.loot_contest, args.loot_contest_peer_drop_every,
        args.inventory_grid, args.inventory_grid_bench,
        args.inventory_grid_review_scale is not None,
        args.inventory_grid_pointer_checkpoint, args.inventory_grid_inspect,
        args.inventory_grid_close_ack, args.inventory_grid_split,
        args.inventory_grid_partial_merge, args.inventory_grid_pickup_hint,
        args.inventory_telemetry, args.animation_review,
        args.combat_hud_checkpoint, args.configure_weapons, args.bench_draft,
        args.weapon_kit, args.weapon_debug, args.clean_art, args.neutral_art,
        args.audio, args.video, args.no_capture,
    ))):
        parser.error("--destruction requires one client, static screenshots and no other review or fixture mode")
    if args.loot_contest_peer_drop_every and not args.loot_contest:
        parser.error("--loot-contest-peer-drop-every requires --loot-contest")
    if args.combat_hud_checkpoint and not (
        args.animation_review and args.clients == 2 and args.viewport is not None
        and not (args.video or args.no_capture or args.weapon_debug or args.voxel_loot)
    ):
        parser.error("--combat-hud-checkpoint requires --animation-review --clients 2 and an explicit --viewport with static capture")
    arena = args.arena_solo or args.arena_pvp
    if args.arena_then_raid and not (arena and args.no_capture):
        parser.error("--arena-then-raid requires --arena-solo or --arena-pvp, with --no-capture")
    if arena and (args.arena_solo and args.arena_pvp
                  or args.clients != (1 if args.arena_solo else 2)
                  or args.pov or args.voxel_loot or args.animation_review
                  or args.weapon_kit or args.configure_weapons or args.video
                  or args.clean_art or args.audio):
        parser.error("Arena routes require the matching client count (solo=1, PvP=2) and no other review mode")
    if args.pet_probe and not args.loot_inspect:
        parser.error("--pet-probe requires --loot-inspect")
    if args.loot_inspect and (not args.voxel_loot or args.no_capture or args.video):
        parser.error("--loot-inspect requires --voxel-loot and static captures (no --no-capture or --video)")
    if args.bench_draft and (not args.configure_weapons or args.no_capture or args.video):
        parser.error("--bench-draft requires --configure-weapons and static captures only")
    grid_review = args.inventory_grid_review_scale is not None
    if args.inventory_grid_pickup_hint and not (args.inventory_grid and not (
        grid_review or args.inventory_grid_bench or args.inventory_grid_close_ack
        or args.inventory_grid_split or args.inventory_grid_partial_merge
        or args.inventory_telemetry or args.loot_inspect or args.configure_weapons
        or args.weapon_kit or args.animation_review or args.pov or args.arena_solo or args.arena_pvp
    )):
        parser.error("--inventory-grid-pickup-hint requires the plain solo --inventory-grid route")
    if args.inventory_grid_partial_merge and not (args.inventory_grid and not (
        grid_review or args.inventory_grid_bench or args.inventory_grid_close_ack
        or args.inventory_grid_split or args.inventory_telemetry or args.loot_inspect
        or args.configure_weapons or args.weapon_kit or args.animation_review
    )):
        parser.error("--inventory-grid-partial-merge requires the plain solo --inventory-grid route")
    if args.inventory_grid_bench and not (
        args.inventory_grid and args.viewport == (1280, 800)
        and not (grid_review or args.inventory_telemetry)
    ):
        parser.error("--inventory-grid-bench requires --inventory-grid --viewport 1280x800 without review or telemetry")
    if args.inventory_grid_close_ack and not (args.inventory_grid and not grid_review
                                               and not args.inventory_telemetry
                                               and not args.inventory_grid_bench):
        parser.error("--inventory-grid-close-ack requires the plain solo --inventory-grid route")
    if args.inventory_grid_split and not (args.inventory_grid and not (
        args.inventory_grid_bench or args.inventory_grid_close_ack or args.inventory_telemetry
    )):
        parser.error("--inventory-grid-split requires solo --inventory-grid without benchmark, close/ACK, or telemetry")
    if args.loot_contest and not (
        args.voxel_loot and args.clients == 2 and args.no_capture and not args.video
        and not args.animation_review and not args.audio and not args.configure_weapons and not args.weapon_kit
    ):
        parser.error("--loot-contest requires plain --voxel-loot --clients 2 --no-capture")
    if args.inventory_grid_inspect and not args.inventory_grid_pointer_checkpoint:
        parser.error("--inventory-grid-inspect requires --inventory-grid-pointer-checkpoint")
    if args.inventory_grid_pointer_checkpoint and not grid_review:
        parser.error("--inventory-grid-pointer-checkpoint requires --inventory-grid-review-scale")
    if grid_review and not (args.inventory_grid and args.viewport and not args.inventory_telemetry
                            and 0.75 <= args.inventory_grid_review_scale <= 2.0):
        parser.error("--inventory-grid-review-scale needs scale 0.75–2.0, --inventory-grid, "
                     "--viewport and no inventory telemetry")
    if args.inventory_telemetry and not (
        sys.platform == "linux" and args.inventory_grid and Path("/proc/self/status").is_file()
    ):
        parser.error("--inventory-telemetry requires Linux procfs and --inventory-grid")
    if args.inventory_grid and not (args.voxel_loot and args.clients == 1 and args.no_capture and not args.video):
        parser.error("--inventory-grid requires --voxel-loot --no-capture with one client and no video")
    if args.voxel_loot and args.pov:
        parser.error("--voxel-loot requires the full raid route")
    crouched_lean_trace = args.pov == "stop-lean" and args.no_capture and args.clients == 2
    if args.no_capture and (args.video or (
        args.pov and args.pov not in TRACE_ONLY_POV and not crouched_lean_trace
    ) or args.weapon_debug):
        parser.error("--no-capture is for the plain raid or animation review, not visual evidence modes")
    if args.audio and not (args.clients == 2 and (args.no_capture or args.pov == "action-fault")):
        parser.error("--audio requires --clients 2 and either --no-capture or --pov action-fault")
    if (args.pov_hz or args.pov_drop_every) and args.pov not in {"stop-lean", "armed-stride", "strafe-reversal"}:
        parser.error("POV cadence/loss options require stop-lean, armed-stride or strafe-reversal")
    if args.pov_peer_lossless and (args.pov not in TRACE_ONLY_POV | {"armed-stride"} or args.clients != 2):
        parser.error("--pov-peer-lossless requires dual armed-stride or a trace-only POV")
    if args.pov in TRACE_ONLY_POV and not (args.clients == 2 and args.no_capture and not args.audio):
        parser.error(f"{args.pov} requires --clients 2 --no-capture without audio")
    if args.pov and (args.animation_review or args.configure_weapons
                     or (args.video and (args.pov != "stop-lean" or args.clients != 1 or args.pov_hz))
                     or (args.clients == 2 and args.pov not in DUAL_POV)):
        parser.error("--pov is standalone; the selected preset does not support those options")
    if args.pov in {"stumble", "action-fault"} and args.clients != 2:
        parser.error(f"{args.pov} requires an observer (--clients 2)")
    assert not args.weapon_debug or args.animation_review, "--weapon-debug requires --animation-review"
    assert not args.clean_art or args.weapon_debug, "--clean-art requires --weapon-debug"
    assert not args.neutral_art or args.clean_art, "--neutral-art requires --clean-art"
    assert args.weapon_debug or args.weapon_lod == "auto", "--weapon-lod requires --weapon-debug"
    assert not (args.configure_weapons and args.weapon_kit), "Panel acceptance must not seed server fixtures"
    root = Path(__file__).resolve().parents[1]
    output = (args.output or root / "target" / "playtest").resolve()
    output.mkdir(parents=True, exist_ok=True)
    if args.pov == "recovery-steps" and any(output.iterdir()):
        parser.error("recovery-steps requires a fresh output directory")
    if args.combat_hud_checkpoint and any(output.glob("combat-hud-*.png")):
        parser.error("--combat-hud-checkpoint needs fresh output for its captures")
    arena_captures = ("arena-eliminated", "arena-respawn", "arena-reentry")
    if args.arena_solo:
        for name in arena_captures:
            (output / f"{name}.png").unlink(missing_ok=True)
    pvp_captures = ("pvp-local-death", "pvp-remote-death", "pvp-both-returned")
    if args.arena_pvp:
        for directory in (output, output / "peer"):
            for name in pvp_captures:
                (directory / f"{name}.png").unlink(missing_ok=True)
    contest_dir = output / "loot-contest"
    if args.loot_contest:
        if contest_dir.exists():
            parser.error("use a fresh output directory for loot contest")
        contest_dir.mkdir()
    grid_review_dir = output / "grid-review"
    if grid_review and grid_review_dir.exists():
        parser.error("use a fresh output directory for grid review")
    if args.pov:
        for name in POV_CAPTURES[args.pov]:
            (output / f"pov-{name}.png").unlink(missing_ok=True)
        (output / "pov-trace.csv").unlink(missing_ok=True)
        if args.clients == 2:
            (output / "peer" / "gait-trace.csv").unlink(missing_ok=True)
            for name in DUAL_POV[args.pov]:
                (output / "peer" / f"pov-{name}.png").unlink(missing_ok=True)
    if args.destruction:
        for cue in DESTRUCTION_CAPTURES:
            (output / f"destruction-{args.destruction}-{cue}.png").unlink(missing_ok=True)
    if args.video:
        assert not (output / "frame-times.csv").exists(), "Use a fresh output directory for video evidence"
    # Old screenshots must never satisfy evidence checks for a new run.
    for name in ("lobby.png", "raid.png", "extracting.png", "debrief.png", "return-lobby.png", "result.txt"):
        (output / name).unlink(missing_ok=True)
    if args.voxel_loot and not args.no_capture:
        for directory in (output, output / "peer"):
            for seed in range(6):
                (directory / f"loot-ground-{seed}.png").unlink(missing_ok=True)
                if args.loot_inspect:
                    for view in ("front", "side", "back"):
                        (directory / f"loot-held-{seed}-{view}.png").unlink(missing_ok=True)
    if not args.no_build:
        # Isolated executable outputs never overwrite a player's running Windows binary.
        for binary in ("server", "client"):
            suffix = ".exe" if os.name == "nt" else ""
            subprocess.run(["cargo", "rustc", "--locked", "-p", binary, "--bin", binary,
                            "--", "-o", str(output / f"{binary}{suffix}")], cwd=root, check=True)
    suffix = ".exe" if os.name == "nt" else ""
    env = os.environ.copy()
    env.pop("TK_FIXTURE_ACTION_FAULT", None)
    env.pop("TK_PLAYTEST_VOXEL_LOOT", None)
    env.pop("TK_PLAYTEST_LOOT_CONTEST", None)
    if args.loot_contest:
        env["TK_PLAYTEST_LOOT_CONTEST"] = str(contest_dir)
    if args.voxel_loot:
        env["TK_PLAYTEST_VOXEL_LOOT"] = "1"
    env.pop("TK_PLAYTEST_LOOT_INSPECT", None)
    if args.loot_inspect:
        env["TK_PLAYTEST_LOOT_INSPECT"] = "1"
    env.pop("TK_PLAYTEST_PET_PROBE", None)
    if args.pet_probe:
        env["TK_PLAYTEST_PET_PROBE"] = args.pet_probe
    env.pop("TK_PLAYTEST_INVENTORY_GRID", None)
    env.pop("TK_PLAYTEST_GRID_SPLIT", None)
    env.pop("TK_PLAYTEST_PARTIAL_MERGE", None)
    env.pop("TK_PLAYTEST_PICKUP_HINT", None)
    if args.inventory_grid_partial_merge:
        env["TK_PLAYTEST_PARTIAL_MERGE"] = "1"
    if args.inventory_grid_split:
        env["TK_PLAYTEST_GRID_SPLIT"] = str(args.inventory_grid_split)
    env.pop("TK_PLAYTEST_GRID_BENCH", None)
    env.pop("TK_PLAYTEST_INVENTORY_TELEMETRY", None)
    env.pop("TK_GRID_REVIEW_DIR", None)
    env.pop("TK_GRID_REVIEW_UI_SCALE", None)
    if args.inventory_grid:
        env["TK_PLAYTEST_INVENTORY_GRID"] = "1"
    if args.inventory_grid_bench:
        env["TK_PLAYTEST_GRID_BENCH"] = "1"
    env.pop("TK_PLAYTEST_WEAPON_KIT", None)
    if args.weapon_kit:
        env["TK_PLAYTEST_WEAPON_KIT"] = args.weapon_kit
    for flag in ("TK_PLAYTEST_VIDEO", "TK_ANIMATION_REVIEW", "TK_PHYSICAL_REVIEW", "TK_DROP_SNAPSHOT_EVERY",
                  "TK_WEAPON_DEBUG", "TK_WEAPON_LOD", "TK_PLAYTEST_CONFIGURATOR", "TK_PLAYTEST_POV",
                  "TK_PLAYTEST_VIEWPORT", "TK_PLAYTEST_POV_HZ", "TK_PLAYTEST_CLEAN_ART",
                 "TK_PLAYTEST_NEUTRAL_ART", "TK_PLAYTEST_NO_CAPTURE", "TK_PLAYTEST_AUDIO",
                 "TK_PLAYTEST_ARENA_SOLO", "TK_PLAYTEST_ARENA_PVP", "TK_PLAYTEST_ARENA_THEN_RAID",
                  "TK_PLAYTEST_DESTRUCTION"):
        env.pop(flag, None)
    if args.arena_solo:
        env["TK_PLAYTEST_ARENA_SOLO"] = "1"
    if args.arena_then_raid:
        env["TK_PLAYTEST_ARENA_THEN_RAID"] = "1"
    if args.arena_pvp:
        env["TK_PLAYTEST_ARENA_PVP"] = "1"
    if args.no_capture:
        env["TK_PLAYTEST_NO_CAPTURE"] = "1"
    if args.audio:
        if "TK_MUTE_AUDIO" in env:
            parser.error("--audio conflicts with TK_MUTE_AUDIO")
        env["TK_PLAYTEST_AUDIO"] = "1"
    if args.pov:
        env["TK_PLAYTEST_POV"] = args.pov
    if args.destruction:
        env["TK_PLAYTEST_DESTRUCTION"] = args.destruction
    env.pop("TK_PLAYTEST_STRAFE_TRACE", None)
    if args.pov == "strafe-reversal":
        env["TK_PLAYTEST_STRAFE_TRACE"] = "1"
    if args.pov == "action-fault":
        env["TK_FIXTURE_ACTION_FAULT"] = "1"
    if args.viewport:
        env["TK_PLAYTEST_VIEWPORT"] = f"{args.viewport[0]}x{args.viewport[1]}"
    if args.pov_hz:
        env["TK_PLAYTEST_POV_HZ"] = str(args.pov_hz)
    env.update(SERVER_ADDR="127.0.0.1", TK_PORT="19080", TK_PLAYTEST_DIR=str(output), TK_PLAYTEST_CLIENTS=str(args.clients), NO_COLOR="1")
    if args.video:
        env["TK_PLAYTEST_VIDEO"] = "1"
    if args.animation_review:
        assert args.clients == 2, "Animation review requires a real UDP observer"
        env["TK_ANIMATION_REVIEW"] = "1"
    env.pop("TK_PLAYTEST_COMBAT_HUD_CHECKPOINT", None)
    if args.combat_hud_checkpoint:
        env["TK_PLAYTEST_COMBAT_HUD_CHECKPOINT"] = "1"
    if args.weapon_debug:
        env["TK_WEAPON_DEBUG"] = "1"
        env["TK_WEAPON_LOD"] = args.weapon_lod
    if args.clean_art:
        env["TK_PLAYTEST_CLEAN_ART"] = "1"
    if args.neutral_art:
        env["TK_PLAYTEST_NEUTRAL_ART"] = "1"
    if args.configure_weapons:
        env["TK_PLAYTEST_CONFIGURATOR"] = "1"
    env.pop("TK_PLAYTEST_BENCH_DRAFT", None)
    bench_views = ("confirmed", "draft", "discarded")
    if args.bench_draft:
        env["TK_PLAYTEST_BENCH_DRAFT"] = "1"
        for view in bench_views:
            (output / f"bench-{view}.png").unlink(missing_ok=True)
    server_path = output / f"server{suffix}"
    client_path = output / f"client{suffix}"
    if args.no_build:
        server_path = server_path if server_path.exists() else root / "target" / "debug" / f"server{suffix}"
        client_path = client_path if client_path.exists() else root / "target" / "debug" / f"client{suffix}"
    if args.server_path:
        server_path = args.server_path.resolve()
    if args.client_path:
        client_path = args.client_path.resolve()
    peer_snapshot_drop_every = (
        (args.loot_contest_peer_drop_every or 0) if args.loot_contest
        else 3 if args.clients == 2 and not args.pov_peer_lossless else 0
    )
    (output / "run-settings.json").write_text(json.dumps({
        "server_executable": str(server_path), "client_executable": str(client_path),
        "server_sha256": hashlib.sha256(server_path.read_bytes()).hexdigest(),
        "client_sha256": hashlib.sha256(client_path.read_bytes()).hexdigest(),
        "clients": args.clients, "transport": "real UDP 127.0.0.1:19080",
        "peer_snapshot_drop_every": peer_snapshot_drop_every,
        "leader_snapshot_drop_every": args.pov_drop_every or 0,
        "resolution": [1280, 800], "world_fov_degrees": 70,
        "requested_viewport": args.viewport,
        "requested_pov_hz": args.pov_hz,
        "video": args.video, "animation_review": args.animation_review,
        "combat_hud_checkpoint": args.combat_hud_checkpoint,
        "no_capture": args.no_capture, "audio": args.audio,
        "loot_inspect": args.loot_inspect,
        "pet_probe": args.pet_probe,
        "inventory_grid": args.inventory_grid,
        "inventory_grid_pickup_hint": args.inventory_grid_pickup_hint,
        "inventory_grid_bench": args.inventory_grid_bench,
        "inventory_grid_review_scale": args.inventory_grid_review_scale,
        "inventory_grid_pointer_checkpoint": args.inventory_grid_pointer_checkpoint,
        "inventory_grid_inspect": args.inventory_grid_inspect,
        "inventory_grid_close_ack": args.inventory_grid_close_ack,
        "inventory_telemetry": args.inventory_telemetry,
        "weapon_debug": args.weapon_debug, "weapon_lod": args.weapon_lod,
        "clean_art": args.clean_art,
        "art_lighting": "neutral white, cast shadows disabled" if args.neutral_art else "raid",
        "initial_weapon_kit_fixture": args.weapon_kit,
        "lobby_configurator": args.configure_weapons,
        "destruction_material": args.destruction,
        "bench_draft": args.bench_draft,
        "capture_target_hz": 30 if args.video else 0,
        "shot_diagnostics": env.get("TK_SHOT_DIAGNOSTICS") is not None,
        "loadouts": ("leader compact M4A1 / peer suppressed M4A1" if args.configure_weapons
                     else "leader Rifle / peer SMG") + "; AP; heavy vest; helmet; both carry pistol",
        "route": (f"destruction-{args.destruction}" if args.destruction
                  else "deploy -> two pickups -> partial merge -> stop" if args.inventory_grid_partial_merge
                  else "deploy -> fragment backpack -> check water HUD -> confirmed Move -> check water HUD"
                  if args.inventory_grid_pickup_hint
                  else "arena-pvp -> leave -> two-player raid deploy" if args.arena_pvp and args.arena_then_raid
                  else "arena-solo -> return -> ordinary raid dead debrief" if args.arena_then_raid
                  else "arena-solo" if args.arena_solo else "arena-pvp" if args.arena_pvp
                  else "deploy -> dual loose item pickup -> stop" if args.loot_contest else args.pov
                  or "optional reciprocal continuous weapon actions -> full-body route -> weapon exercise -> loot -> sentry combat -> extraction -> debrief -> lobby -> deploy -> limb injury/fall/recovery/death"),
        "timestamps": "client.log PLAYTEST stage clocks and ANIMATION CONTACT local clocks; frame-times.csv real frame timestamps",
    }, indent=2), encoding="utf-8")
    with (output / "server.log").open("w", encoding="utf-8") as server_log:
        server_env = env | ({"TK_PLAYTEST_INVENTORY_TELEMETRY": "1"} if args.inventory_telemetry else {})
        server = subprocess.Popen([str(server_path)], cwd=root, env=server_env, stdout=server_log, stderr=subprocess.STDOUT)
        try:
            processes = []
            logs = []
            try:
                for index in range(args.clients):
                    client_output = output if index == 0 else output / "peer"
                    client_output.mkdir(exist_ok=True)
                    client_env = env.copy()
                    client_env.update(TK_PLAYTEST_DIR=str(client_output), TK_PLAYTEST_ROLE=str(index))
                    if args.inventory_grid_pickup_hint:
                        client_env["TK_PLAYTEST_PICKUP_HINT"] = "1"
                    if index == 1 and peer_snapshot_drop_every:
                        client_env["TK_DROP_SNAPSHOT_EVERY"] = str(peer_snapshot_drop_every)
                    elif index == 0 and args.pov == "recovery-steps" and peer_snapshot_drop_every:
                        # The observer is selected by accepted actor ID, not
                        # process index. Apply the same drop rule to both.
                        client_env["TK_DROP_SNAPSHOT_EVERY"] = str(peer_snapshot_drop_every)
                    elif index == 0 and args.pov_drop_every:
                        client_env["TK_DROP_SNAPSHOT_EVERY"] = str(args.pov_drop_every)
                    log = (client_output / "client.log").open("w", encoding="utf-8")
                    logs.append(log)
                    client_args = [str(client_path), "--playtest"]
                    if args.inventory_grid_close_ack:
                        client_args.append("--inventory-grid-close-ack")
                    if grid_review:
                        client_args.append("--inventory-grid-review")
                        if args.inventory_grid_pointer_checkpoint:
                            client_args.append("--inventory-grid-pointer-checkpoint")
                        if args.inventory_grid_inspect:
                            client_args.append("--inventory-grid-inspect")
                        client_env.update(TK_GRID_REVIEW_DIR=str(grid_review_dir),
                                          TK_GRID_REVIEW_UI_SCALE=str(args.inventory_grid_review_scale))
                    processes.append(subprocess.Popen(client_args, cwd=root,
                                                      env=client_env, stdout=log, stderr=subprocess.STDOUT))
                if args.inventory_telemetry:
                    memory_samples = sample_solo_memory(processes[0], server)
                for process in processes:
                    code = process.wait(timeout=150 if args.destruction else 45 if args.pov else 110 if arena else 300)
                    if code != 0:
                        raise AssertionError(f"Client failed ({code}); see {output}")
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=10)
                for log in logs:
                    log.close()
            if server.poll() is not None:
                raise AssertionError(f"Server exited unexpectedly ({server.returncode}); see {output / 'server.log'}")
        finally:
            # Terminate only the process owned by this harness, never an existing game server.
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
    client_text = (output / "client.log").read_text(encoding="utf-8", errors="replace")
    settings = json.loads((output / "run-settings.json").read_text())
    if not arena:
        settings.update(json.loads((output / "camera-settings.json").read_text()))
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    server_text = (output / "server.log").read_text(encoding="utf-8", errors="replace")
    combined = re.sub(r"\x1b\[[0-9;]*m", "", client_text + server_text)
    assert "panicked at" not in combined, f"Panic in logs: {output}"
    assert " ERROR " not in combined, f"Runtime/render error in logs: {output}"
    if args.destruction:
        proof = re.search(rf"DESTRUCTION PASS {args.destruction} t=([0-9.]+)", client_text)
        assert proof, f"Missing confirmed destruction completion: {output / 'client.log'}"
        for cue in DESTRUCTION_CAPTURES:
            name = f"destruction-{args.destruction}-{cue}"
            assert f"DESTRUCTION EVIDENCE {name} " in client_text
            data = (output / f"{name}.png").read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
            width, height = struct.unpack(">II", data[16:24])
            assert width >= 600 and height >= 400
            assert args.viewport is None or args.viewport == (width, height), (
                f"{name}.png: expected {args.viewport}, got {(width, height)}; "
                "check whether the window manager overrides the requested size"
            )
            assert cue == DESTRUCTION_CAPTURES[0] or settings["resolution"] == [width, height]
            settings["resolution"] = [width, height]
        settings["seconds_to_evidence"] = float(proof[1])
        (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
        result = f"PASS: native {args.destruction} damage/fragments, real UDP, named screenshots at {proof[1]}s; temporal/artistic judgement remains manual\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.pov == "recovery-steps":
        traces = list(output.rglob("recovery-steps.json"))
        assert len(traces) == 1, f"Missing or ambiguous observer proof: {output}"
        recovery = json.loads(traces[0].read_text())
        assert 0 < recovery["fall"] < recovery["recovering"] < recovery["standing"]
        assert recovery["distance_m"] > 0.5 and recovery["attached_ids"]
        first, second = recovery["supports"]
        assert {first["side"], second["side"]} == {0, 1}
        assert recovery["standing"] < first["confirmed_start"] < second["confirmed_start"]
        for support in (first, second):
            assert support["confirmed_start"] <= support["presented_start"] <= support["presented_end"] < support["confirmed_end"]
            assert support["pairs"] >= 2 and 0 < support["max_frame_seconds"] < 0.05
        assert not list(output.rglob("*.png")), "Telemetry-only route produced media"
        walker = output if traces[0].parent != output else output / "peer"
        counts = {}
        for directory, cue in ((walker, "POV PASS"), (traces[0].parent, "POV OBSERVER PASS")):
            text = (directory / "client.log").read_text()
            assert "panicked at" not in text and " ERROR " not in text
            assert f"{cue} recovery-steps" in text
            dropped = re.findall(r"SNAPSHOT LOSS dropped=(\d+) sequence=(\d+)", text)
            assert bool(dropped) != args.pov_peer_lossless and all(int(seq) % 3 == 0 for _, seq in dropped)
            counts[directory.relative_to(output).as_posix()] = int(dropped[-1][0]) if dropped else 0
        fixture = re.findall(r"RECOVERY FIXTURE raid=(\d+) actor=(\d+) life=(\d+)", server_text)
        assert fixture == [tuple(str(recovery[key]) for key in ("raid", "actor", "life"))]
        settings["recovery_steps"] = recovery
        settings["recovery_snapshot_drop_logged_counts"] = counts
        (output / "run-settings.json").write_text(json.dumps(settings, indent=2))
        result = "PASS: recovery-steps, two native UDP clients, first two alternating supports\n"
        (output / "result.txt").write_text(result)
        print(result, end="")
        return
    if args.arena_pvp:
        handoffs = []
        for role, directory in enumerate((output, output / "peer")):
            text = (directory / "client.log").read_text(encoding="utf-8", errors="replace")
            assert "panicked at" not in text and " ERROR " not in text, directory
            cues = [f"ARENA PVP DEATH role={role} target={i} life=0" for i in range(2)]
            cues += [f"ARENA PVP RETURNS role={role} lives=1/1"]
            if args.arena_then_raid:
                cues += [f"ARENA PVP RAID LOBBY role={role}", f"ARENA PVP RAID PASS role={role}"]
                checkpoints = re.findall(
                    rf"ARENA PVP RAID PASS role={role} arena=(\d+) raid=(\d+) ids=(\d+)/(\d+) snapshot=\d+",
                    text,
                )
                assert len(checkpoints) == 1, f"Missing or repeated handoff: {directory}"
                handoffs.append(checkpoints[0])
            else:
                cues += ["ARENA PVP PASS"]
            offsets = [text.index(cue) for cue in cues]
            assert offsets == sorted(offsets), f"Unexpected PvP transitions: {directory}"
            if not args.no_capture:
                for name in pvp_captures:
                    data = (directory / f"{name}.png").read_bytes()
                    assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
                    if args.viewport:
                        assert struct.unpack(">II", data[16:24]) == args.viewport
        if args.arena_then_raid:
            assert handoffs[0] == handoffs[1], f"Clients disagree on handoff epoch or roster: {output}"
            assert not list(output.rglob("*.png")), f"No-capture route produced media: {output}"
            result = "PASS: two native UDP clients kept their identities through Arena PvP -> ordinary raid\n"
        else:
            result = "PASS: native Arena PvP, real UDP, both deaths and returns, clean lobby after disconnect\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.arena_solo:
        cues = ["BOT DEATH", "BOT RETURN", "PLAYER DEATH", "PLAYER RETURN"]
        cues += (["RAID handoff", "LOBBY CLEAN", "RAID active", "RAID dead", "RAID NO RESPAWN PASS"]
                 if args.arena_then_raid else ["LOBBY CLEAN", "SOLO PASS"])
        offsets = [client_text.index(f"ARENA {cue}") for cue in cues]
        assert offsets == sorted(offsets), f"Unexpected Arena transitions: {output}"
        if not args.no_capture:
            for name in arena_captures:
                data = (output / f"{name}.png").read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
                if args.viewport:
                    assert struct.unpack(">II", data[16:24]) == args.viewport
        if args.arena_then_raid:
            assert not list(output.rglob("*.png")), f"No-capture route produced media: {output}"
            result = "PASS: native solo UDP Arena return -> ordinary raid dead debrief without respawn\n"
        else:
            result = "PASS: native Arena Solo, real UDP, both combatants returned and second session began\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.inventory_grid_pickup_hint:
        cues = ["PICKUP HINT before ", "PICKUP HINT move ", "PICKUP HINT after ", "PICKUP HINT PASS"]
        assert all(client_text.count(cue) == 1 for cue in cues), f"Missing or repeated HUD checkpoint: {output}"
        offsets = [client_text.index(cue) for cue in cues]
        assert offsets == sorted(offsets), f"Unexpected HUD checkpoint order: {output}"
        assert not list(output.rglob("*.png")), f"No-capture route produced media: {output}"
        result = "PASS: native solo UDP pickup HUD shows no-room then restores [F] after a confirmed Move\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.inventory_grid_partial_merge:
        report = partial_merge_evidence(client_text)
        (output / "evidence.json").write_text(json.dumps({"partial_merge": report}, indent=2), encoding="utf-8")
        result = "PASS: native UDP partial merge, 2+2 → 1+3 (see evidence.json)\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.loot_contest:
        peer_text = (output / "peer" / "client.log").read_text(encoding="utf-8", errors="replace")
        assert "panicked at" not in peer_text and " ERROR " not in peer_text, f"Peer runtime error: {output}"
        report = loot_contest_evidence((client_text, peer_text), server_text)
        dropped = re.findall(r"SNAPSHOT LOSS dropped=(\d+) sequence=(\d+)", peer_text)
        every = args.loot_contest_peer_drop_every
        if every:
            assert dropped and all(int(seq) % every == 0 for _, seq in dropped), (
                "Missing or inconsistent contest snapshot-loss evidence"
            )
        else:
            assert not dropped, "Unexpected contest snapshot discard"
        report["peer_snapshot_drop_every"] = every or 0
        report["peer_snapshot_drop_logged_count"] = int(dropped[-1][0]) if dropped else 0
        (contest_dir / "evidence.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        result = f"PASS: native dual loot contest; item={report['item']} owner={report['winner']}\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    if args.pov:
        if args.pov_drop_every:
            dropped = re.findall(r"SNAPSHOT LOSS dropped=(\d+) sequence=(\d+)", client_text)
            assert dropped, "Missing snapshot discard evidence on the POV leader"
            assert all(int(seq) % args.pov_drop_every == 0 for _, seq in dropped)
            settings["leader_snapshot_drop_logged_count"] = int(dropped[-1][0])
        if args.clients == 2:
            peer = output / "peer"
            peer_text = (peer / "client.log").read_text(encoding="utf-8", errors="replace")
            peer_text = re.sub(r"\x1b\[[0-9;]*m", "", peer_text)
            assert "POV OBSERVER PASS" in peer_text and "panicked at" not in peer_text
            assert " ERROR " not in peer_text, f"Runtime/render error in observer log: {peer}"
            if args.pov == "stop-lean":
                settings["observer_gait"] = gait_metrics(peer / "gait-trace.csv")
            else:
                dropped = re.findall(r"SNAPSHOT LOSS dropped=(\d+) sequence=(\d+)", peer_text)
                assert bool(dropped) != args.pov_peer_lossless, "Unexpected observer loss mode"
                assert all(int(seq) % 3 == 0 for _, seq in dropped)
                settings["observer_snapshot_drop_logged_count"] = int(dropped[-1][0]) if dropped else 0
                if args.pov == "armed-stride":
                    assert (peer / "gait-trace.csv").stat().st_size > 500
            if args.pov == "prone":
                edges = re.findall(r"POV PRONE EDGE sequence=(\d+) pos=\[([^\]]+)\] confirmed_prone=(true|false)", client_text)
                assert len(edges) == 3 and [edge[2] for edge in edges] == ["false", "true", "true"]
                assert 7.9 < float(edges[1][1].split(",")[-1]) < 8.35, "Blocked edge was not beneath canopy"
                assert float(edges[2][1].split(",")[-1]) < 6.8, "Final edge was not clear of canopy"
                for role, text in (("LEADER", client_text), ("PEER", peer_text)):
                    for cue in ("prone", "refused", "return"):
                        expected = "false" if cue == "return" else "true"
                        assert f"POV PRONE {role} {'pov-' if role == 'LEADER' else ''}{cue} prone={expected} " in text
            if args.pov == "stumble":
                for role, text, prefix in (("LEADER", client_text, "pov-"), ("PEER", peer_text, "")):
                    cues = re.findall(rf"POV STUMBLE {role} {prefix}(\S+) mode=(\S+)", text)
                    expected = [("stumble", "Stumbling"), ("contact", "Standing"), ("return", "Standing")]
                    if role == "LEADER":
                        expected.insert(0, ("rest", "Standing"))
                    assert cues == expected, f"Unexpected stumble checkpoints on {role}: {cues}"
            if args.pov == "west-step":
                keys = ("actor", "foot", "floor", "confirmed", "presented",
                        "floor_ms", "confirmed_ms", "presented_ms")
                samples = re.findall(r"WEST STEP CONTACT " + " ".join(
                    rf"{key}=(\d+)" for key in keys), peer_text)
                assert len(samples) == 1, f"Same-foot step contact not observed exactly once: {peer}"
                contact = dict(zip(keys, map(int, samples[0]), strict=True))
                assert contact["foot"] in (0, 1)
                assert contact["floor"] < contact["confirmed"] <= contact["presented"]
                assert contact["floor_ms"] < contact["confirmed_ms"] <= contact["presented_ms"]
                settings["west_step_contact"] = contact
            if args.pov == "action-fault":
                for remote, kind, text in (("false", "local", client_text), ("true", "spatial", peer_text)):
                    cues = re.findall(r"POV FAULT CUE remote=(true|false) scheduled=(true|false)", text)
                    assert cues == [(remote, str(args.audio).lower())], f"Expected one fault cue, got {cues}"
                    sinks = re.findall(r"PLAYTEST FAULT AUDIO SINK (\S+)", text)
                    assert sinks == ([kind] if args.audio else []), f"Unexpected fault playback sinks: {sinks}"
            for name in DUAL_POV[args.pov]:
                assert f"POV OBSERVER {name} " in peer_text, f"Missing observer cue {name}: {peer}"
                if crouched_lean_trace:
                    continue
                data = (peer / f"pov-{name}.png").read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
                if args.viewport:
                    assert struct.unpack(">II", data[16:24]) == args.viewport
        if args.pov_hz:
            trace = output / "pov-trace.csv"
            assert trace.is_file() and trace.stat().st_size > 500, f"Missing camera cadence trace: {trace}"
        proof = re.search(rf"POV PASS {re.escape(args.pov)} t=([0-9.]+)", client_text)
        assert proof, f"Missing POV completion proof: {output / 'client.log'}"
        names = tuple(f"pov-{name}" for name in POV_CAPTURES[args.pov])
        for name in names:
            assert f"POV EVIDENCE {name} " in client_text
            if crouched_lean_trace:
                continue
            data = (output / f"{name}.png").read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
            width, height = struct.unpack(">II", data[16:24])
            assert width >= 600 and height >= 400
            assert args.viewport is None or args.viewport == (width, height), (
                f"{name}.png: expected {args.viewport}, got {(width, height)}; "
                "check whether the window manager overrides the requested size"
            )
            assert name == names[0] or settings["resolution"] == [width, height]
            settings["resolution"] = [width, height]
        settings["seconds_to_evidence"] = float(proof[1])
        if args.pov == "strafe-reversal":
            settings["observer_reversal"] = strafe_reversal.analyze(
                strafe_reversal.read_trace(peer / "strafe-reversal.csv"))
        if crouched_lean_trace:
            settings["observer_crouched_lean"] = crouched_lean.analyze(
                crouched_lean.read_trace(peer / "crouched-lean.csv"))
        (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
        if args.video:
            subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], check=True)
        if args.pov == "strafe-reversal":
            measurement = settings["observer_reversal"]
            result = f"STRAFE REVERSAL: {measurement['coverage']}; route complete, see run-settings.json\n"
        elif crouched_lean_trace:
            measurement = settings["observer_crouched_lean"]
            result = f"CROUCHED LEAN: {measurement['coverage']}; route complete, see run-settings.json\n"
        else:
            result = f"PASS: native POV {args.pov}, real UDP, evidence at {proof[1]}s\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        if (args.pov == "strafe-reversal" or crouched_lean_trace) and measurement["coverage"] != "measured":
            return 1
        return
    assert "PLAYTEST PASS" in client_text, f"Missing completion proof: {output / 'client.log'}"
    if args.configure_weapons:
        assert "INITIAL EQUIPMENT FIXTURE" not in server_text
        assert "LOBBY CONFIGURATOR PASS peer=false capacity=20" in client_text
        assert "REDEPLOY CONFIGURATOR PASS capacity=20" in client_text
    if args.weapon_kit:
        assert f"INITIAL EQUIPMENT FIXTURE {args.weapon_kit}" in server_text
        capacity, definitions = {
            "compact": (20, [120, 122, 124, 126, 127, 130, 131]),
            "suppressed": (30, [101, 122, 123, 125, 128, 130, 131, 129, 132, 133, 134]),
            "afg": (20, [120, 122, 123, 126, 128, 130, 131, 135]),
        }[args.weapon_kit]
        assert f"DEPLOY KIT capacity={capacity} definitions={definitions}" in client_text, "Wrong replicated kit"
    if args.clients == 2:
        peer_text = (output / "peer" / "client.log").read_text(encoding="utf-8", errors="replace")
        assert "PLAYTEST PASS" in peer_text, "Peer did not finish the normal raid cycle"
        if args.configure_weapons:
            assert "LOBBY CONFIGURATOR PASS peer=true capacity=30" in peer_text
            assert "REDEPLOY CONFIGURATOR PASS capacity=30" in peer_text
        assert "panicked at" not in peer_text, "Peer panic"
        assert "SNAPSHOT LOSS" in peer_text, "Missing deliberate peer snapshot loss"
        for text in (client_text, peer_text):
            assert "WEAPON EXERCISE PASS" in text, "Missing weapon action evidence"
            assert "PHYSICAL PLAYTEST PASS" in text, "Missing physical scenario evidence"
            assert "ANATOMY RESET" in text, "Missing new raid anatomy/physics reset"
            assert "PRESENTATION CONVERGED" in text, "Physical render-entity convergence checker did not run"
            assert "CONFIRMED FLASH EVIDENCE" in text, "Missing visible confirmed muzzle flash evidence"
            assert "LIVE RECOVERY PASS" in text, "Missing articulated fall/get-up continuity proof"
            if args.animation_review:
                assert "ADS EVIDENCE m1911-ads" in text and "ADS EVIDENCE primary-ads" in text, "Missing authoritative ADS enter/release proof"
                assert "RELOAD MECHANISM EVIDENCE m1911-slide-release" in text and "RELOAD MECHANISM EVIDENCE primary-bolt-release" in text, "Missing empty-reload mechanism proof"
        recovery = [json.loads((directory / "live-recovery.json").read_text()) for directory in (output, output / "peer")]
        assert recovery[0] == recovery[1], "Clients disagree on surviving fall/recovery IDs"
        assert recovery[0]["control_restored"] and recovery[0]["fall_ids"] == recovery[0]["getup_ids"]
        convergence = r"CONVERGENCE victim=(\d+) missing=(\d+) wounds=(\d+) organs=(\[[^\]]+\]) pieces=(\[[^\]]+\])"
        leader_state = re.search(convergence, client_text)
        peer_state = re.search(convergence, peer_text)
        assert leader_state and peer_state, "Missing final converged state logs"
        assert leader_state.groups() == peer_state.groups(), "Clients disagree on persistent victim/organ/wound/piece state"
    if args.voxel_loot:
        loot_logs = [client_text] + ([peer_text] if args.clients == 2 else [])
        if args.inventory_grid:
            assert "GRID PROBE PASS move/rotate and split/merge" in client_text, "Missing confirmed grid checkpoint"
        if args.inventory_grid_split:
            splits = re.findall(
                r"GRID VARIABLE SPLIT confirmed quantity=(\d+) child=(\d+) source=(\d+) revision=(\d+)",
                client_text,
            )
            assert len(splits) == 1, splits
            quantity, child, source, revision = map(int, splits[0])
            assert quantity == args.inventory_grid_split and child != source and revision > 0, splits
        records = re.findall(r"VOXEL LOOT (collected|dropped|retained) seed=(\d+) id=(\d+)", "\n".join(loot_logs))
        acquired = sorted(int(seed) for action, seed, _ in records if action != "dropped")
        assert acquired == list(range(6)), f"Missing or duplicate voxel acquisitions: {records}"
        assert sorted((seed, item) for action, seed, item in records if action == "collected") == sorted(
            (seed, item) for action, seed, item in records if action == "dropped"
        ), "Discard did not preserve each acquired instance"
        assert sum(action == "retained" for action, _, _ in records) == args.clients
        assert "collected loot" not in server_text, "Voxel route collected a legacy crate"
        if args.loot_inspect:
            inspected = re.findall(r"VOXEL INSPECT seed=(\d+) id=(\d+) view=(front|side|back)", "\n".join(loot_logs))
            expected = [(seed, item, view)
                        for action, seed, item in records if action != "dropped"
                        for view in ("front", "side", "back")]
            assert sorted(inspected) == sorted(expected), f"Missing or mismatched inspection evidence: {inspected}"
    milestones = ("extracted; secured=1", "debrief") if args.voxel_loot else (
        "collected loot", "extracted; secured=1", "debrief"
    )
    for milestone in milestones:
        assert milestone in server_text, f"Missing authoritative milestone: {milestone}"
    if args.no_capture:
        logs = [client_text] + ([peer_text] if args.clients == 2 else [])
        for text in logs:
            for kind in ("local", "spatial"):
                assert (f"PLAYTEST AUDIO SINK {kind}" in text) == args.audio, (
                    f"Unexpected or missing {kind} audio playback evidence"
                )
        evidence = {
            "clients": args.clients, "captures": [],
            "server_performance": re.findall(r"TK PERF [^\r\n]+", server_text),
            "client_performance": re.findall(r"TK PERF [^\r\n]+", client_text),
            "peer_performance": re.findall(r"TK PERF [^\r\n]+", logs[1]) if len(logs) == 2 else [],
        }
        assert evidence["client_performance"], "Missing frame time evidence"
        if args.inventory_grid_bench:
            evidence["inventory_frame_windows"] = inventory_frame_windows.analyze(client_text.splitlines())
        if args.inventory_grid_close_ack:
            diagnostic = grid_close_ack_evidence(client_text)
            evidence["grid_close_ack"] = diagnostic
        if args.inventory_telemetry:
            assert all(any(row[name] for row in memory_samples) for name in ("client", "server")), (
                "Missing process memory samples"
            )
            evidence["inventory_channel_application_payload"] = inventory_payload_totals(server_text)
            evidence["linux_process_memory_samples"] = memory_samples
        if grid_review:
            review = grid_review_evidence(grid_review_dir, client_text, args.viewport,
                                          args.inventory_grid_review_scale,
                                          pointer_checkpoint=args.inventory_grid_pointer_checkpoint,
                                          inspect=args.inventory_grid_inspect)
            evidence["grid_review"] = review
            evidence["captures"] = [
                f"grid-review/{row['png']}"
                for row in [*review["observed"].values(), *review.get("inspection", [])] if row["png"]
            ]
        (output / "evidence.json").write_text(json.dumps(evidence, indent=2), encoding="utf-8")
        if args.inventory_grid_close_ack:
            result = f"GRID CLOSE ACK: {diagnostic['status']} (solo raid completed; see evidence.json)\n"
        elif grid_review:
            assert not review["findings"], "\n".join(review["findings"])
            if args.inventory_grid_split:
                assert review["observed"].get("split-quantity", {}).get("split") == {
                    "item": source, "source_quantity": 3, "quantity": quantity, "min": 1, "max": 2,
                }, "The submitted split quantity was not observed in the rendered chooser"
            result = "PASS: native UDP raid with partial static grid evidence (see evidence.json)\n"
        else:
            result = f"PASS: native UDP raid without framebuffer capture; audio={args.audio}\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    evidence = []
    resolution = None
    for name in ("lobby", "raid", "extracting", "debrief", "return-lobby"):
        path = output / f"{name}.png"
        data = path.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n", f"Not a real PNG: {path}"
        width, height = struct.unpack(">II", data[16:24])
        assert width >= 600 and height >= 400 and len(data) > 5000, f"Invalid capture: {path}"
        assert args.viewport is None or (width, height) == args.viewport, f"Unexpected viewport: {path}"
        if resolution is None:
            resolution = [width, height]
        assert [width, height] == resolution, f"Native viewport changed during the run: {path}"
        evidence.append(f"{name}.png: {width}x{height}, {len(data)} bytes")
    settings["resolution"] = resolution
    if args.combat_hud_checkpoint:
        checkpoints = re.findall(
            r"COMBAT HUD CHECKPOINT (present|absent) item=(\d+) snapshot=(\d+)", client_text)
        assert [state for state, _, _ in checkpoints] == ["present", "absent"], checkpoints
        (_, item, first), (_, other, last) = checkpoints
        assert item == other and 0 < int(first) < int(last), checkpoints
        switches = re.findall(
            r"COMBAT HUD SWITCH from=(\d+) to=(\d+) before=(\d+) snapshot=(\d+) "
            r"chamber=(loaded|empty) magazine=(\d+|absent|unknown)", client_text)
        assert len(switches) == 1, switches
        source, target, before, snapshot, chamber, magazine = switches[0]
        assert source == item and target != source and 0 < int(before) < int(snapshot), switches
        settings["combat_hud_switch"] = {
            "from_item": int(source), "to_item": int(target), "before_snapshot": int(before),
            "snapshot": int(snapshot), "chamber": chamber, "magazine": magazine,
        }
        cleared = re.findall(r"COMBAT HUD CLEARED (debrief|lobby) snapshot=(\d+)", client_text)
        assert [phase for phase, _ in cleared] == ["debrief", "lobby"], cleared
        debrief, lobby = (int(seq) for _, seq in cleared)
        assert int(last) < debrief < lobby, cleared
        settings["combat_hud_cleared"] = {"debrief": debrief, "lobby": lobby}
        for name in ("combat-hud-magazine-present", "combat-hud-magazine-absent", "combat-hud-switch"):
            path = output / f"{name}.png"
            data = path.read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, path
            width, height = struct.unpack(">II", data[16:24])
            assert [width, height] == resolution, f"HUD capture viewport mismatch: {path}"
            evidence.append(f"{path.name}: {width}x{height}, {len(data)} bytes")
    if args.bench_draft:
        reviewed = re.findall(r"BENCH REVIEW (confirmed|draft|discarded) handle=(\d+) marker=0 focused=true", client_text)
        assert tuple(view for view, _ in reviewed) == bench_views, "Missing or repeated workbench stages"
        assert len({handle for _, handle in reviewed}) == 1, "Draft or discard changed the confirmed handle"
        for view in bench_views:
            path = output / f"bench-{view}.png"
            data = path.read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing workbench capture: {path}"
            width, height = struct.unpack(">II", data[16:24])
            assert [width, height] == resolution, f"Workbench viewport changed: {path}"
            evidence.append(f"{path.name}: {width}x{height}, {len(data)} bytes")
    if args.voxel_loot:
        for seed in range(6):
            directory = output / "peer" if args.clients == 2 and seed in (2, 3, 5) else output
            path = directory / f"loot-ground-{seed}.png"
            data = path.read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing ground-item capture: {path}"
            width, height = struct.unpack(">II", data[16:24])
            assert width >= 600 and height >= 400, f"Invalid capture: {path}"
            evidence.append(f"{path.relative_to(output)}: {width}x{height}, {len(data)} bytes")
            if args.loot_inspect:
                for view in ("front", "side", "back"):
                    held = directory / f"loot-held-{seed}-{view}.png"
                    data = held.read_bytes()
                    assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing held-item capture: {held}"
                    width, height = struct.unpack(">II", data[16:24])
                    assert width >= 600 and height >= 400, f"Invalid capture: {held}"
                    evidence.append(f"{held.relative_to(output)}: {width}x{height}, {len(data)} bytes")
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    if args.clients == 2:
        names = ["pose-idle", "pose-walk", "pose-run", "pose-strafe", "pose-crouch", "pose-jump", "pose-land", "pose-stop",
                 "weapon-burst", "weapon-empty", "weapon-reload", "weapon-pistol", "weapon-pistol-reload", "weapon-pistol-ready", "weapon-primary-reload",
                 "injury-healthy", "injury-correction", "injury-lateral-limp", "injury-arm", "injury-separation", "injury-fall", "injury-recovery", "injury-ragdoll"]
        for directory in (output, output / "peer"):
            if args.weapon_debug:
                for label in ("right", "left", "front", "top", "three-quarter"):
                    path = directory / f"asset-{label}.png"
                    data = path.read_bytes()
                    assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing live asset view: {path}"
                    assert args.viewport is None or struct.unpack(">II", data[16:24]) == args.viewport, (
                        f"Unexpected viewport: {path}"
                    )
            for name in names:
                path = directory / f"{name}.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing native evidence: {path}"
            if args.configure_weapons:
                kit = "compact" if directory == output else "suppressed"
                path = directory / f"configurator-{kit}.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing confirmed panel preview: {path}"
            primary = "m4a1" if directory == output or args.configure_weapons else "mp5"
            for weapon in (primary, "m1911"):
                path = directory / f"confirmed-{weapon}-flash.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing confirmed flash capture: {path}"
        evidence.append("50 additional native pose/weapon/injury/confirmed-flash frames; identical final anatomy/piece IDs; peer drops every third snapshot")
    (output / "evidence.json").write_text(json.dumps({"clients": args.clients, "captures": evidence,
        "server_performance": re.findall(r"TK PERF [^\r\n]+", server_text),
        "client_performance": re.findall(r"TK PERF [^\r\n]+", client_text)}, indent=2), encoding="utf-8")
    report = "\n".join([
        f"PASS: {args.clients} native rendered client(s) + real UDP server",
        "Lobby -> countdown -> combat -> loot -> extraction -> debrief -> lobby -> second raid",
        *evidence,
        f"Server milestones: {'; '.join(milestones)}",
        "Client assertions and exact positions: client.log",
    ])
    (output / "result.txt").write_text(report + "\n", encoding="utf-8")
    print(report)
    if args.video:
        subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], check=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
