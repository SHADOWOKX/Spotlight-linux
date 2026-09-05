import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Controller} from './controller.js';

export default class SpotlightIntegration extends Extension {
    enable() {
        if (typeof Meta.Window.prototype.hide_from_window_list !== 'function' ||
            typeof Meta.Window.prototype.show_in_window_list !== 'function')
            throw new Error('Spotlight integration requires GNOME 50 window-list APIs');
        this.controller = new Controller(global.display, Main.wm,
            global.get_window_actors().map(actor => actor.meta_window));
    }

    disable() {
        this.controller?.disable();
        this.controller = null;
    }
}
