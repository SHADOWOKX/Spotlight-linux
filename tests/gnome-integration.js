import {Controller} from '../integrations/gnome/spotlight@shadowokx/controller.js';

function assert(value) {
    if (!value)
        throw new Error('GNOME integration regression');
}
class Signals {
    constructor() { this.signals = new Map(); this.next = 1; }
    connect(name, callback) { const id = this.next++; this.signals.set(id, [name, callback]); return id; }
    disconnect(id) { this.signals.delete(id); }
    emit(name, ...args) {
        for (const [signal, callback] of [...this.signals.values()])
            if (name === signal) callback(this, ...args);
    }
}
class Window extends Signals {
    constructor(title, app = 'io.github.shadowokx.SpotlightLinux', hidden = false) {
        super(); this.title = title; this.app = app; this.hidden = hidden;
    }
    get_gtk_application_id() { return this.app; }
    get_title() { return this.title; }
    is_skip_taskbar() { return this.hidden; }
    hide_from_window_list() { this.hidden = true; }
    show_in_window_list() { this.hidden = false; }
}
const display = new Signals();
const original = function () { assert(this === wm); return true; };
const wm = {_shouldAnimateActor: original};
const palette = new Window('Spotlight Linux');
const settings = new Window('Spotlight Linux Settings');
const other = new Window('Spotlight Linux', 'other.app');
const prehidden = new Window('Spotlight Linux', undefined, true);
const controller = new Controller(display, wm, [palette, settings, other, prehidden]);
assert(palette.hidden && !settings.hidden && !other.hidden);
assert(!wm._shouldAnimateActor({meta_window: palette}));
assert(wm._shouldAnimateActor({meta_window: settings}));
assert(wm._shouldAnimateActor({meta_window: other}));
const late = new Window('');
display.emit('window-created', late);
late.title = 'Spotlight Linux'; late.emit('notify::title');
assert(late.hidden);
late.emit('unmanaged'); assert(late.signals.size === 0);
palette.title = 'Spotlight Linux Settings'; palette.emit('notify::title');
assert(!palette.hidden);
palette.title = 'Spotlight Linux'; palette.emit('notify::title');
const wrapper = wm._shouldAnimateActor;
controller.disable();
assert(!palette.hidden && prehidden.hidden && wm._shouldAnimateActor === original);
assert(display.signals.size === 0 && palette.signals.size === 0);
assert(wrapper.call(wm, {meta_window: palette}));
print('PASS: scoped matching, late identity, unchanged Settings/other apps, cleanup and restoration');
