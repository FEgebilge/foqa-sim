// FOQA-Sim weather setter: puts a challenge's weather into the sim through
// the sim's own weather service (JS_LISTENER_WEATHER, docs/challenges.md).
// Plain ES5, no sim dependency in the helpers (node test/foqa-weather.test.js).
// bus: { on(name, cb(json)), send(name, json) }.
var FoqaWeather = (function () {
    "use strict";

    var MSG = { set: "FOQA_SIM_WEATHER", cmd: "FOQA_SIM_CMD" };
    var WAIT_MS = 5000, SETTLE_MS = 4000;
    // Tolerances of the read-back (preset) and of the SimVars (the sim blends).
    var TOL_PRESET = { deg: 0.5, kt: 0.1, ft: 1, hpa: 0.1, c: 0.1 };
    var TOL_SIMVAR = { deg: 10, kt: 2, hpa: 1.5, c: 2.5 };
    var ISA_LAPSE_C_PER_FT = 0.00198;
    var GUST_OFF = 1;   // gustWaveData.dvSpeedMultiplier with no gust (to verify in flight)

    // Linear units: factor into the kind's base unit (kt, m, hPa, deg).
    var LINEAR = {
        speed: { kt: 1, kts: 1, knot: 1, knots: 1, "m/s": 1.943844, mps: 1.943844, meterspersecond: 1.943844,
            "km/h": 0.539957, kmh: 0.539957, kph: 0.539957, kilometersperhour: 0.539957 },
        length: { ft: 0.3048, foot: 0.3048, feet: 0.3048, m: 1, meter: 1, meters: 1, metre: 1, metres: 1 },
        pressure: { hpa: 1, mbar: 1, mb: 1, millibar: 1, millibars: 1, inhg: 33.8639 },
        angle: { deg: 1, degree: 1, degrees: 1, rad: 180 / Math.PI, radian: 180 / Math.PI, radians: 180 / Math.PI }
    };
    var CELSIUS = { c: 1, celsius: 1, degc: 1 }, FAHRENHEIT = { f: 1, fahrenheit: 1, degf: 1 };

    function norm(u) { return String(u == null ? "" : u).toLowerCase().replace(/[°\s.]/g, ""); }

    function has(table, key) { return Object.prototype.hasOwnProperty.call(table, key); }

    // value in `from` -> `to`; an unknown unit throws (fail closed).
    function convert(kind, value, from, to) {
        var a = norm(from), b = norm(to);
        if (kind === "temp") {
            [[a, from], [b, to]].forEach(function (u) {
                if (!has(CELSIUS, u[0]) && !has(FAHRENHEIT, u[0])) throw new Error("unknown temp unit: " + u[1]);
            });
            var c = has(FAHRENHEIT, a) ? (value - 32) * 5 / 9 : value;
            return has(FAHRENHEIT, b) ? c * 9 / 5 + 32 : c;
        }
        var t = LINEAR[kind];
        if (!t || !has(t, a)) throw new Error("unknown " + kind + " unit: " + from);
        if (!has(t, b)) throw new Error("unknown " + kind + " unit: " + to);
        return value * t[a] / t[b];
    }

    // A RangeDataValue as a number in `to`, and a number written into one.
    function read(field, kind, to) { return convert(kind, field.value, field.unit, to); }
    function write(field, kind, value, from) { field.value = convert(kind, value, from, field.unit); }

    function angDiff(a, b) { var d = Math.abs(a - b) % 360; return d > 180 ? 360 - d : d; }

    function num(v) { return typeof v === "number" && isFinite(v); }

    // Spec check; null when fine.
    function validate(spec) {
        if (!spec || typeof spec !== "object") return "no spec";
        if (spec.base !== "clear" && !(typeof spec.base === "number" && spec.base >= 0 && spec.base % 1 === 0)) return "bad base";
        if (!Array.isArray(spec.wind) || !spec.wind.length) return "wind needs layers";
        for (var i = 0; i < spec.wind.length; i++) {
            var w = spec.wind[i];
            if (!w || !num(w.alt_ft) || !num(w.from_deg) || !num(w.kt) || w.kt < 0 || w.from_deg < 0 || w.from_deg > 360) return "bad wind layer " + i;
        }
        if (spec.qnh_hpa != null && !(spec.qnh_hpa >= 850 && spec.qnh_hpa <= 1085)) return "bad qnh_hpa";
        if (spec.temp_c != null && !(spec.temp_c >= -80 && spec.temp_c <= 60)) return "bad temp_c";
        if (spec.clouds && spec.clouds.length) return "clouds not supported (the base preset's stay)";
        return null;
    }

    // The preset with the spec in it, a deep copy; throws on an unknown unit
    // or more wind layers than the preset has. Layers past the spec's repeat
    // its last one (altitudes stay the preset's). Gusts are off everywhere.
    function edit(preset, spec) {
        var p = JSON.parse(JSON.stringify(preset)), layers = p.tWindLayers || [];
        if (spec.wind.length > layers.length) throw new Error("preset has " + layers.length + " wind layers, spec " + spec.wind.length);
        layers.forEach(function (l, i) {
            var w = spec.wind[Math.min(i, spec.wind.length - 1)];
            if (i < spec.wind.length) write(l.dvAltitude, "length", w.alt_ft, "ft");
            write(l.dvAngleRad, "angle", w.from_deg, "deg");
            write(l.dvSpeed, "speed", w.kt, "kt");
            var g = l.gustWaveData && l.gustWaveData.dvSpeedMultiplier;
            if (g) g.value = GUST_OFF;
        });
        if (spec.qnh_hpa != null) write(p.oSettings.dvMSLPressure, "pressure", spec.qnh_hpa, "hPa");
        if (spec.temp_c != null) write(p.oSettings.dvMSLGLTemperature, "temp", spec.temp_c, "c");
        return p;
    }

    // What a preset says, in spec units.
    function reading(preset) {
        var o = preset.oSettings || {};
        return {
            wind: (preset.tWindLayers || []).map(function (l) {
                return { alt_ft: read(l.dvAltitude, "length", "ft"), from_deg: read(l.dvAngleRad, "angle", "deg"), kt: read(l.dvSpeed, "speed", "kt") };
            }),
            qnh_hpa: o.dvMSLPressure ? read(o.dvMSLPressure, "pressure", "hPa") : null,
            temp_c: o.dvMSLGLTemperature ? read(o.dvMSLGLTemperature, "temp", "c") : null
        };
    }

    // Spec vs the echoed preset: strings, none when it is all there.
    function compare(spec, preset) {
        var bad = [], r = reading(preset);
        spec.wind.forEach(function (w, i) {
            var m = r.wind[i];
            if (!m) return bad.push("wind " + i + " missing");
            if (Math.abs(m.alt_ft - w.alt_ft) > TOL_PRESET.ft) bad.push("wind " + i + " alt " + m.alt_ft.toFixed(0) + " ft, set " + w.alt_ft);
            if (angDiff(m.from_deg, w.from_deg) > TOL_PRESET.deg) bad.push("wind " + i + " dir " + m.from_deg.toFixed(1) + ", set " + w.from_deg);
            if (Math.abs(m.kt - w.kt) > TOL_PRESET.kt) bad.push("wind " + i + " speed " + m.kt.toFixed(1) + " kt, set " + w.kt);
        });
        (preset.tWindLayers || []).forEach(function (l, i) {
            var g = l.gustWaveData && l.gustWaveData.dvSpeedMultiplier;
            if (g && g.value !== GUST_OFF) bad.push("wind " + i + " gust multiplier " + g.value);
        });
        if (spec.qnh_hpa != null && (r.qnh_hpa == null || Math.abs(r.qnh_hpa - spec.qnh_hpa) > TOL_PRESET.hpa)) bad.push("qnh " + r.qnh_hpa + ", set " + spec.qnh_hpa);
        if (spec.temp_c != null && (r.temp_c == null || Math.abs(r.temp_c - spec.temp_c) > TOL_PRESET.c)) bad.push("temp " + r.temp_c + ", set " + spec.temp_c);
        return bad;
    }

    // The spec's wind at an altitude: between two layers interpolated
    // (direction the short way), beyond the ends the nearest.
    function windAt(layers, alt_ft) {
        var s = layers.slice().sort(function (a, b) { return a.alt_ft - b.alt_ft; });
        if (alt_ft <= s[0].alt_ft) return { from_deg: s[0].from_deg, kt: s[0].kt };
        for (var i = 1; i < s.length; i++) {
            if (alt_ft <= s[i].alt_ft) {
                var a = s[i - 1], b = s[i], f = (alt_ft - a.alt_ft) / (b.alt_ft - a.alt_ft);
                var d = ((b.from_deg - a.from_deg + 540) % 360) - 180;
                return { from_deg: (a.from_deg + d * f + 360) % 360, kt: a.kt + (b.kt - a.kt) * f };
            }
        }
        var l = s[s.length - 1];
        return { from_deg: l.from_deg, kt: l.kt };
    }

    // Spec vs SimVars m = { wind_dir_deg, wind_kt, qnh_hpa, temp_c, alt_ft }.
    function compareMeasured(spec, m) {
        var bad = [], w = windAt(spec.wind, m.alt_ft || 0);
        if (w.kt > 3 && angDiff(m.wind_dir_deg, w.from_deg) > TOL_SIMVAR.deg) bad.push("sim wind dir " + m.wind_dir_deg.toFixed(0) + ", set " + w.from_deg.toFixed(0));
        if (Math.abs(m.wind_kt - w.kt) > TOL_SIMVAR.kt) bad.push("sim wind " + m.wind_kt.toFixed(1) + " kt, set " + w.kt.toFixed(1));
        if (spec.qnh_hpa != null && Math.abs(m.qnh_hpa - spec.qnh_hpa) > TOL_SIMVAR.hpa) bad.push("sim qnh " + m.qnh_hpa.toFixed(1) + ", set " + spec.qnh_hpa);
        if (spec.temp_c != null) {
            var want = spec.temp_c - ISA_LAPSE_C_PER_FT * (m.alt_ft || 0);
            if (Math.abs(m.temp_c - want) > TOL_SIMVAR.c) bad.push("sim temp " + m.temp_c.toFixed(1) + ", expected " + want.toFixed(1));
        }
        return bad;
    }

    // Index of the first preset whose entry mentions `needle`, else -1.
    function findPreset(list, needle) {
        for (var i = 0; i < list.length; i++) {
            var it = list[i], hay = typeof it === "string" ? it : JSON.stringify(it);
            if (hay.toUpperCase().indexOf(needle) >= 0) return it && typeof it.index === "number" ? it.index : i;
        }
        return -1;
    }

    // ---- the sim side ----

    function Setter(env) {
        this.env = env || {};
        this.listener = null;
        this.list = null;
        this.preset = null;
        this.seq = 0;      // UpdatePreset events heard
        this.busy = false;
    }

    Setter.prototype.open = function () {
        if (this.listener) return;
        var self = this, register = this.env.register ||
            (typeof RegisterViewListenerT === "function" ? RegisterViewListenerT : null);
        if (!register) throw new Error("no view listener support");
        this.listener = register("JS_LISTENER_WEATHER", function () {}, "ViewListener.ViewListener");
        this.listener.on("SetWeatherList", function (l) { self.list = l; });
        this.listener.on("UpdatePreset", function (p) { self.preset = p; self.seq++; });
    };

    Setter.prototype.until = function (test, ms, cb) {
        var t0 = Date.now();
        (function poll() {
            if (test()) return cb(true);
            if (Date.now() - t0 > ms) return cb(false);
            setTimeout(poll, 50);
        })();
    };

    Setter.prototype.simvars = function () {
        var g = this.env.simvar || function (n, u) { return SimVar.GetSimVarValue(n, u); };
        return {
            wind_dir_deg: g("AMBIENT WIND DIRECTION", "degrees"), wind_kt: g("AMBIENT WIND VELOCITY", "knots"),
            qnh_hpa: g("SEA LEVEL PRESSURE", "millibars"), temp_c: g("AMBIENT TEMPERATURE", "celsius"),
            alt_ft: g("PLANE ALTITUDE", "feet")
        };
    };

    // done({ ok, applied, measured, mismatches[], error? })
    Setter.prototype.apply = function (spec, done) {
        var self = this, fin = false;
        function end(res) { if (!fin) { fin = true; self.busy = false; done(res); } }
        function fail(msg) { end({ ok: false, applied: null, measured: null, mismatches: [], error: msg }); }
        var bad = validate(spec);
        if (bad) return done({ ok: false, applied: null, measured: null, mismatches: [], error: bad });
        if (this.busy) return done({ ok: false, applied: null, measured: null, mismatches: [], error: "busy" });
        this.busy = true;
        try { this.open(); } catch (e) { return fail(String(e.message || e)); }
        var L = this.listener;
        if (!this.list) L.trigger("ASK_WEATHER_LIST");
        this.until(function () { return self.list; }, WAIT_MS, function (gotList) {
            if (!gotList) return fail("no weather list");
            var index = spec.base === "clear" ? findPreset(self.list, "CLEAR_SKY") : spec.base;
            if (index < 0) return fail("no CLEAR_SKY preset in the list");
            var seq = self.seq;
            L.trigger("CUSTOM_WEATHER_PRESET_CHANGED", index);
            self.until(function () { return self.seq > seq; }, WAIT_MS, function (gotPreset) {
                if (!gotPreset) return fail("preset " + index + " not sent");
                var edited;
                try { edited = edit(self.preset, spec); } catch (e) { return fail(String(e.message || e)); }
                var before = self.seq;
                Promise.resolve(L.call("UpdateTempWeatherPreset", edited)).then(function (res) {
                    self.until(function () { return self.seq > before; }, 3000, function (echoed) {
                        var back = echoed ? self.preset : (res && res.tWindLayers ? res : null);
                        if (!back) return fail("no echo of the preset");
                        var applied, mismatches;
                        try { applied = reading(back); mismatches = compare(spec, back); } catch (e) { return fail(String(e.message || e)); }
                        setTimeout(function () {
                            var measured;
                            try { measured = self.simvars(); mismatches = mismatches.concat(compareMeasured(spec, measured)); }
                            catch (e) { measured = null; mismatches.push("simvars: " + String(e.message || e)); }
                            end({ ok: mismatches.length === 0, applied: applied, measured: measured, mismatches: mismatches });
                        }, self.env.settleMs == null ? SETTLE_MS : self.env.settleMs);
                    });
                }, function (err) { fail("UpdateTempWeatherPreset: " + String(err && err.message || err)); });
            });
        });
    };

    // Answers FOQA_SIM_WEATHER { v:2, cmd:"set_weather", req, spec } with
    // FOQA_SIM_CMD { v:2, cmd:"weather_result", req, ...report }, sent three
    // times a second apart (the app listens only some of the time). The app
    // repeats its message, so a req is handled once.
    function attach(bus, env) {
        var setter = new Setter(env), seen = {};
        bus.on(MSG.set, function (json) {
            var m;
            try { m = JSON.parse(json); } catch (e) { return; }
            if (!m || m.v !== 2 || m.cmd !== "set_weather" || typeof m.req !== "string" || seen[m.req]) return;
            seen[m.req] = true;
            setter.apply(m.spec, function (report) {
                report.v = 2; report.cmd = "weather_result"; report.req = m.req;
                var text = JSON.stringify(report);
                for (var i = 0; i < 3; i++) setTimeout(function () { bus.send(MSG.cmd, text); }, i * 1000);
            });
        });
        return setter;
    }

    var shared = null;
    var api = {
        MSG: MSG, attach: attach, Setter: Setter, convert: convert, validate: validate, edit: edit, reading: reading,
        compare: compare, compareMeasured: compareMeasured, windAt: windAt, findPreset: findPreset,
        apply: function (spec, done) { (shared || (shared = new Setter())).apply(spec, done); }
    };
    if (typeof module !== "undefined" && module.exports) module.exports = api;
    return api;
})();
