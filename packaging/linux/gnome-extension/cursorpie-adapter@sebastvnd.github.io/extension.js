// CursorPie adapter for GNOME Shell.
//
// Wayland does not let a program ask where the pointer is or which windows there are; GNOME Shell
// is the only one that knows. This extension tells CursorPie, and does nothing else:
//
//  * it reports the pointer and the window list (ids, app names, positions: never titles) to
//    CursorPie's D-Bus name org.cursorpie.Wayland, when something changes and when CursorPie asks;
//  * it answers one call, org.cursorpie.GnomeBridge.Sync, which makes it report now;
//  * it brings a window forward when CursorPie's reply says so ("activate:<id>"), and reads the
//    pointer at full rate while CursorPie says a hold is running ("arm" ... "disarm").
//
// It reads no file, runs no program and listens on no port. The reply of CursorPie is split on ";"
// and only "arm", "disarm" and "activate:<digits>" are understood.
//
// Written from the GNOME Shell extension guide (gjs.guide) and not yet run on a GNOME session.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const SERVICE = 'org.cursorpie.Wayland';
const PATH = '/org/cursorpie/Wayland';
const IFACE = 'org.cursorpie.Wayland';

const BRIDGE_NAME = 'org.cursorpie.GnomeBridge';
const BRIDGE_PATH = '/org/cursorpie/GnomeBridge';
const BRIDGE_XML = `
<node>
  <interface name="org.cursorpie.GnomeBridge">
    <method name="Sync"/>
  </interface>
</node>`;

// How often the pointer is read during a hold, how long a hold may last before the extension stops
// on its own (CursorPie's own "disarm" may have been lost), the least time between two reports
// caused by a change, and between two caused by Sync.
const POINTER_MS = 16;
const ARM_LIMIT_MS = 30000;
const REPORT_GAP_MS = 200;
const SYNC_GAP_MS = 20;
const MAX_WINDOWS = 400;

function nowMs() {
    return GLib.get_monotonic_time() / 1000;
}

export default class CursorPieAdapter extends Extension {
    enable() {
        this._destroyed = false;
        this._armedAt = 0;
        this._pointerTimer = 0;
        this._reportTimer = 0;
        this._lastReport = 0;
        this._lastSync = 0;
        this._signals = [];
        this._ownerId = 0;
        this._bridge = null;

        const display = global.display;
        this._connect(display, 'notify::focus-window', () => this._reportSoon());
        this._connect(display, 'window-created', () => this._reportSoon());
        this._connect(display, 'restacked', () => this._reportSoon());
        this._connect(global.window_manager, 'destroy', () => this._reportSoon());
        this._connect(global.workspace_manager, 'active-workspace-changed', () => this._reportSoon());

        try {
            this._bridge = Gio.DBusExportedObject.wrapJSObject(BRIDGE_XML, this);
            this._ownerId = Gio.bus_own_name(
                Gio.BusType.SESSION,
                BRIDGE_NAME,
                Gio.BusNameOwnerFlags.NONE,
                connection => {
                    try {
                        this._bridge?.export(connection, BRIDGE_PATH);
                    } catch (e) {
                        console.warn(`CursorPie adapter: could not export the bridge: ${e.message}`);
                    }
                },
                () => this._report(),
                () => {});
        } catch (e) {
            console.warn(`CursorPie adapter: could not own ${BRIDGE_NAME}: ${e.message}`);
        }
    }

    disable() {
        this._destroyed = true;
        for (const [obj, id] of this._signals ?? []) {
            try {
                obj.disconnect(id);
            } catch (e) {
                // the object may be gone already
            }
        }
        this._signals = null;
        this._removeTimer('_pointerTimer');
        this._removeTimer('_reportTimer');
        if (this._bridge) {
            try {
                this._bridge.unexport();
            } catch (e) {
                // not exported
            }
            this._bridge = null;
        }
        if (this._ownerId) {
            Gio.bus_unown_name(this._ownerId);
            this._ownerId = 0;
        }
    }

    // The one method that other programs on the session bus may call (see BRIDGE_XML).
    Sync() {
        const now = nowMs();
        if (this._destroyed || now - this._lastSync < SYNC_GAP_MS)
            return;
        this._lastSync = now;
        this._report();
    }

    _connect(obj, name, callback) {
        try {
            this._signals.push([obj, obj.connect(name, callback)]);
        } catch (e) {
            // a signal this version of GNOME Shell does not have: the report just comes less often
        }
    }

    _removeTimer(field) {
        if (this[field]) {
            GLib.Source.remove(this[field]);
            this[field] = 0;
        }
    }

    // ------------------------------------------------------------------------- what is reported

    _windows() {
        let list = [];
        try {
            list = global.get_window_actors()
                .map(actor => actor.meta_window)
                .filter(w => w);
        } catch (e) {
            return [];
        }
        try {
            // bottom to top
            list = global.display.sort_windows_by_stacking(list);
        } catch (e) {
            // the order of the actors is the stacking order too, as far as it is documented
        }
        return list;
    }

    _isApplicationWindow(w) {
        const t = w.get_window_type();
        return t === Meta.WindowType.NORMAL ||
            t === Meta.WindowType.DIALOG ||
            t === Meta.WindowType.MODAL_DIALOG ||
            t === Meta.WindowType.UTILITY;
    }

    _desktopOf(w) {
        try {
            if (w.is_on_all_workspaces())
                return 0;
            const ws = w.get_workspace();
            return ws ? ws.index() + 1 : 0;
        } catch (e) {
            return 0;
        }
    }

    // The desktop-entry id of the window's app, without ".desktop" (CursorPie matches both).
    _desktopFileOf(w, tracker) {
        let id = '';
        try {
            id = w.get_gtk_application_id() ?? '';
        } catch (e) {
            id = '';
        }
        if (!id) {
            try {
                id = tracker.get_window_app(w)?.get_id() ?? '';
            } catch (e) {
                id = '';
            }
        }
        return id.endsWith('.desktop') ? id.slice(0, -'.desktop'.length) : id;
    }

    _snapshot() {
        const tracker = Shell.WindowTracker.get_default();
        const list = this._windows();
        const wins = [];
        // top-most first
        for (let i = list.length - 1; i >= 0 && wins.length < MAX_WINDOWS; i--) {
            const w = list[i];
            let r;
            try {
                r = w.get_frame_rect();
            } catch (e) {
                continue;
            }
            wins.push({
                id: String(w.get_id()),
                cls: w.get_wm_class() ?? '',
                name: w.get_wm_class_instance() ?? '',
                df: this._desktopFileOf(w, tracker),
                x: r.x,
                y: r.y,
                w: r.width,
                h: r.height,
                min: !!w.minimized,
                skip: !this._isApplicationWindow(w),
                desk: this._desktopOf(w),
            });
        }

        const outs = [];
        try {
            for (const m of Main.layoutManager.monitors)
                outs.push({x: m.x, y: m.y, w: m.width, h: m.height});
        } catch (e) {
            // no outputs: CursorPie keeps what XWayland says
        }

        let desk = 0;
        try {
            desk = global.workspace_manager.get_active_workspace_index() + 1;
        } catch (e) {
            desk = 0;
        }
        const focus = global.display.focus_window;
        const [px, py] = global.get_pointer();
        return {
            px,
            py,
            active: focus ? String(focus.get_id()) : '',
            desk,
            wins,
            outs,
        };
    }

    // ------------------------------------------------------------------------------- reporting

    _report() {
        if (this._destroyed)
            return;
        this._lastReport = nowMs();
        let json;
        try {
            json = JSON.stringify(this._snapshot());
        } catch (e) {
            console.warn(`CursorPie adapter: could not read the windows: ${e.message}`);
            return;
        }
        Gio.DBus.session.call(
            SERVICE, PATH, IFACE, 'Report',
            new GLib.Variant('(s)', [json]),
            new GLib.VariantType('(s)'),
            Gio.DBusCallFlags.NO_AUTO_START, 500, null,
            (connection, result) => {
                try {
                    const [commands] = connection.call_finish(result).deepUnpack();
                    this._apply(commands);
                } catch (e) {
                    // CursorPie is not running, or went away: nothing to tell
                }
            });
    }

    // A change happened: report, but not more often than every REPORT_GAP_MS. A change inside the gap
    // is reported when the gap is over, so the last state is never missed.
    _reportSoon() {
        if (this._destroyed || this._reportTimer)
            return;
        const wait = REPORT_GAP_MS - (nowMs() - this._lastReport);
        if (wait <= 0) {
            this._report();
            return;
        }
        this._reportTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, Math.ceil(wait), () => {
            this._reportTimer = 0;
            this._report();
            return GLib.SOURCE_REMOVE;
        });
    }

    _apply(commands) {
        if (this._destroyed || typeof commands !== 'string')
            return;
        for (const c of commands.split(';')) {
            if (c === 'arm') {
                this._arm();
            } else if (c === 'disarm') {
                this._armedAt = 0;
            } else if (c.startsWith('activate:')) {
                const id = c.substring('activate:'.length);
                if (/^[0-9]{1,20}$/.test(id))
                    this._activate(id);
            }
        }
    }

    _activate(id) {
        for (const w of this._windows()) {
            if (String(w.get_id()) === id) {
                // un-minimizes, goes to the window's workspace and raises it
                Main.activateWindow(w, global.get_current_time());
                return;
            }
        }
    }

    _arm() {
        this._armedAt = nowMs();
        if (this._pointerTimer)
            return;
        this._pointerTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, POINTER_MS, () => {
            if (this._destroyed || !this._armedAt || nowMs() - this._armedAt > ARM_LIMIT_MS) {
                this._armedAt = 0;
                this._pointerTimer = 0;
                return GLib.SOURCE_REMOVE;
            }
            const [x, y] = global.get_pointer();
            Gio.DBus.session.call(
                SERVICE, PATH, IFACE, 'Pointer',
                new GLib.Variant('(s)', [`${x},${y}`]),
                null,
                Gio.DBusCallFlags.NO_AUTO_START, 200, null, null);
            return GLib.SOURCE_CONTINUE;
        });
    }
}
