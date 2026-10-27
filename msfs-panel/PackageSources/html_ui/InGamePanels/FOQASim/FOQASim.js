// MSFS toolbar glue: the panel's custom element hands its content element
// and the sim's Comm Bus (listen, and call SimConnect clients such as the
// FOQA-Sim app) to the shared board (foqa-board.js).
class FoqaSimPanel extends TemplateElement {
    connectedCallback() {
        super.connectedCallback();
        if (this.board) return;
        const bus = RegisterCommBusListener(() => {});
        this.board = FoqaSim.attach(this.querySelector("#foqa"), {
            on: (name, cb) => bus.on(name, cb),
            send: (name, json) => bus.callSimConnect(name, json)
        });
    }
}
window.customElements.define("foqa-sim-panel", FoqaSimPanel);
checkAutoload();
