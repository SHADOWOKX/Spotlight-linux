export function isPalette(window) {
    return window?.get_gtk_application_id() === 'io.github.shadowokx.SpotlightLinux' &&
        window.get_title() === 'Spotlight Linux';
}

// No timers, keybindings, subprocesses, IPC endpoints or global setting changes.
export class Controller {
    constructor(display, wm, windows) {
        this.display = display;
        this.wm = wm;
        this.windows = new Map();
        this.enabled = true;
        if (typeof wm._shouldAnimateActor !== 'function')
            throw new Error('Unsupported GNOME window animation API');
        this.original = wm._shouldAnimateActor;
        const controller = this;
        this.wrapper = function (actor, ...args) {
            if (controller.enabled && isPalette(actor?.meta_window))
                return false;
            return controller.original.call(this, actor, ...args);
        };
        wm._shouldAnimateActor = this.wrapper;
        this.createdId = display.connect('window-created', (_display, window) => this.track(window));
        for (const window of windows)
            this.track(window);
    }

    track(window) {
        if (this.windows.has(window))
            return;
        const record = {ids: [], hidden: false};
        this.windows.set(window, record);
        const sync = () => {
            if (isPalette(window)) {
                if (!record.hidden && !window.is_skip_taskbar()) {
                    window.hide_from_window_list();
                    record.hidden = true;
                }
            } else if (record.hidden) {
                window.show_in_window_list();
                record.hidden = false;
            }
        };
        record.ids.push(window.connect('notify::gtk-application-id', sync));
        record.ids.push(window.connect('notify::title', sync));
        record.ids.push(window.connect('unmanaged', () => this.forget(window, false)));
        sync();
    }

    forget(window, restore) {
        const record = this.windows.get(window);
        if (!record)
            return;
        for (const id of record.ids)
            window.disconnect(id);
        if (restore && record.hidden)
            window.show_in_window_list();
        this.windows.delete(window);
    }

    disable() {
        this.enabled = false;
        this.display.disconnect(this.createdId);
        // Do not overwrite another extension's later wrapper. If it chains
        // ours, the disabled wrapper simply delegates to the original method.
        if (this.wm._shouldAnimateActor === this.wrapper)
            this.wm._shouldAnimateActor = this.original;
        for (const window of [...this.windows.keys()])
            this.forget(window, true);
    }
}
