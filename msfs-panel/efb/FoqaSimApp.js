// MSFS 2024 EFB glue: registers the FOQA-Sim tablet app with the EFB OS and
// shows two tabs, Live (foqa-board.js) and Flights (foqa-history.js), over
// one Comm Bus listener. build.ps1 puts both ahead of this file in one
// script. Plain JS against the OS's globals: window.EFB_API (the app
// container) and msfssdk (FSComponent, DisplayComponent, Subject).
(function () {
    "use strict";

    var NAME = "FoqaSimApp";    // = folder and file name, no whitespace
    var VERSION = "0.1.0";
    var BASE_URL = "coui://html_ui/efb_ui/efb_apps/" + NAME;
    var BUS_WAIT_MS = 3000;
    var sdk = msfssdk, FSComponent = sdk.FSComponent;

    // The EFB page does not load CommBus.js; without it the views stay
    // "not connected".
    function loadCommBus() {
        return new Promise(function (resolve) {
            if (typeof RegisterCommBusListener === "function") return resolve();
            Include.addScript("/JS/Services/CommBus.js", resolve);
            setTimeout(resolve, BUS_WAIT_MS);
        });
    }

    function commBus() {
        if (typeof RegisterCommBusListener !== "function") return { on: function () {} };
        var listener = RegisterCommBusListener(function () {});
        return {
            on: function (name, cb) { listener.on(name, cb); },
            send: function (name, json) { listener.callSimConnect(name, json); }
        };
    }

    // Tabs over the two views; the history asks for the list when shown.
    function Shell(root, bus) {
        var self = this;
        root.innerHTML = '<div class="efb-tabs"><span class="efb-tab" data-tab="live">Live</span>' +
            '<span class="efb-tab" data-tab="flights">Flights</span></div>' +
            '<div class="efb-pane" data-pane="live"></div><div class="efb-pane" data-pane="flights"></div>';
        this.root = root;
        this.board = FoqaSim.attach(root.querySelector('[data-pane="live"]'), bus);
        this.history = FoqaHistory.attach(root.querySelector('[data-pane="flights"]'), bus);
        FoqaWeather.attach(bus);   // idle until the app sends set_weather
        root.querySelector(".efb-tabs").addEventListener("click", function (e) {
            var tab = e.target.getAttribute("data-tab");
            if (tab) self.show(tab);
        });
        this.show("live");
    }

    Shell.prototype.show = function (tab) {
        this.tab = tab;
        Array.prototype.forEach.call(this.root.querySelectorAll("[data-tab],[data-pane]"), function (el) {
            el.classList.toggle("on", (el.getAttribute("data-tab") || el.getAttribute("data-pane")) === tab);
        });
        if (tab === "flights") this.history.show();
    };

    // What the OS drives: onOpen once, onResume/onPause on app switches,
    // onUpdate every frame, destroy when killed.
    class FoqaSimView extends sdk.DisplayComponent {
        constructor(props) {
            super(props);
            this.ref = FSComponent.createRef();
            this.pageKeyActions = new Map();
        }
        render() { return FSComponent.buildComponent("div", { ref: this.ref, class: "foqa-efb" }); }
        onAfterRender() { this.shell = new Shell(this.ref.instance, this.props.commBus); }
        onOpen() {}
        onClose() {}
        onResume() { if (this.shell.tab === "flights") this.shell.history.show(); }
        onPause() {}
        onUpdate() {}
        routeGamepadInteractionEvent() {}
        handlePageKeyAction() {}
    }

    // The members EFB_API.use() and the OS read of an app (the SDK's App
    // class is bundled into each app, not exposed).
    class FoqaSimApp {
        constructor() {
            this.available = sdk.Subject.create(true);
            this.BootMode = 0;      // AppBootMode.COLD: rendered on first open
            this.SuspendMode = 0;   // AppSuspendMode.SLEEP: kept, and listening, while closed
            this._isReady = false;
            this._favoriteIndex = -1;
            this.options = {};
        }
        get name() { return "FOQA-Sim"; }
        get internalName() { return NAME; }
        get icon() { return BASE_URL + "/Assets/app-icon.svg"; }
        get isReady() { return this._isReady; }
        get favoriteIndex() { return this._favoriteIndex; }
        set favoriteIndex(i) { this._favoriteIndex = i; }
        get compatibleAircraftModels() { return undefined; }
        getIsSearchable() { return true; }
        getIsFavoritable() { return true; }
        getVersion() { return VERSION; }
        async _install(props) {
            this.bus = props.bus;
            this.options = props.options || {};
            await this.install();
            this._isReady = true;
            Coherent.trigger("EFB_APP_INSTALLED", this.name, this.internalName);
        }
        async install() {
            EFB_API.loadCss(BASE_URL + "/" + NAME + ".css").catch(function () {});
            await loadCommBus();
            this.commBus = commBus();
        }
        render() { return FSComponent.buildComponent(FoqaSimView, { commBus: this.commBus }); }
    }

    EFB_API.use(FoqaSimApp);
})();
