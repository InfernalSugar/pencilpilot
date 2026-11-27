#!/usr/bin/env python3
"""Pointer input for the app on a headless mutter, through mutter's RemoteDesktop D-Bus API
(the session bus of the `dbus-run-session` mutter runs in). Nothing reaches a real screen.

    python3 -I inject.py hover <seconds> <x1> <y1> <x2> <y2> [interval-ms]

hover: put the pointer at (x1, y1) (from the screen's top-left), then move it between (x1, y1)
and (x2, y2) every interval (2 ms by default), so a widget under each point is hovered in turn:
the app has something new to draw every display frame.
"""

import sys
import time

import dbus

BUS = "org.gnome.Mutter.RemoteDesktop"


def session():
    bus = dbus.SessionBus()
    rd = bus.get_object(BUS, "/org/gnome/Mutter/RemoteDesktop")
    path = dbus.Interface(rd, BUS).CreateSession()
    s = dbus.Interface(bus.get_object(BUS, path), BUS + ".Session")
    s.Start()
    return s


def move(s, dx, dy):
    s.NotifyPointerMotionRelative(dbus.Double(dx), dbus.Double(dy))


def main():
    if len(sys.argv) < 7 or sys.argv[1] != "hover":
        print(__doc__)
        sys.exit(2)
    secs, x1, y1, x2, y2 = (float(v) for v in sys.argv[2:7])
    interval = float(sys.argv[7]) / 1000 if len(sys.argv) > 7 else 0.002
    s = session()
    # to the top-left corner (motion is clamped to the screen), then to the first point
    move(s, -100000, -100000)
    move(s, x1, y1)
    t0 = time.monotonic()
    n = 0
    at_first = True
    while time.monotonic() - t0 < secs:
        if at_first:
            move(s, x2 - x1, y2 - y1)
        else:
            move(s, x1 - x2, y1 - y2)
        at_first = not at_first
        n += 1
        time.sleep(interval)
    s.Stop()
    print(f"{n} moves in {time.monotonic() - t0:.2f} s", file=sys.stderr)


if __name__ == "__main__":
    main()
