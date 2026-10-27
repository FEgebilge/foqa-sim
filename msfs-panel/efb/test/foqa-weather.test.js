// node msfs-panel/efb/test/foqa-weather.test.js
var assert = require("assert");
var W = require("../foqa-weather.js");

function near(a, b, tol) { assert.ok(Math.abs(a - b) <= (tol || 1e-6), a + " vs " + b); }

// units
near(W.convert("speed", 18, "kt", "m/s"), 9.2600, 1e-3);
near(W.convert("speed", 36, "km/h", "kt"), 19.4384, 1e-3);
near(W.convert("speed", 10, "knots", "kph"), 18.52, 1e-2);
near(W.convert("length", 1000, "ft", "m"), 304.8);
near(W.convert("length", 304.8, "m", "ft"), 1000);
near(W.convert("pressure", 29.92, "inHg", "hPa"), 1013.2, 0.1);
near(W.convert("pressure", 1013.25, "mbar", "hPa"), 1013.25);
near(W.convert("temp", 59, "°F", "c"), 15);
near(W.convert("temp", 15, "°C", "°F"), 59);
near(W.convert("temp", -40, "F", "C"), -40);
near(W.convert("angle", Math.PI, "radians", "deg"), 180);
assert.throws(function () { W.convert("speed", 1, "furlongs", "kt"); }, /unknown speed unit/);
assert.throws(function () { W.convert("temp", 1, "K", "c"); }, /unknown temp unit/);
assert.throws(function () { W.convert("pressure", 1, "", "hPa"); }, /unknown pressure unit/);

// preset in the user's units: inHg, F, ft, kt, radians
function rv(value, unit) { return { value: value, unit: unit }; }
function layer(alt) {
    return { dvAltitude: rv(alt, "ft"), dvAngleRad: rv(0, "radians"), dvSpeed: rv(5, "kt"),
        gustWaveData: { dvSpeedMultiplier: rv(2.5, "number") } };
}
function preset() {
    return { tWindLayers: [layer(0), layer(3000), layer(10000)], tCloudLayers: [],
        oSettings: { dvMSLPressure: rv(29.92, "inHg"), dvMSLGLTemperature: rv(59, "°F") } };
}
var spec = { base: "clear", wind: [{ alt_ft: 0, from_deg: 270, kt: 18 }, { alt_ft: 3000, from_deg: 280, kt: 25 }], qnh_hpa: 1020, temp_c: 10 };

assert.strictEqual(W.validate(spec), null);
assert.ok(W.validate({ base: "clear", wind: [] }));
assert.ok(W.validate({ base: "storm", wind: spec.wind }));
assert.ok(W.validate({ base: 2, wind: [{ alt_ft: 0, from_deg: 400, kt: 1 }] }));
assert.ok(W.validate({ base: 2, wind: spec.wind, clouds: [{}] }));
assert.strictEqual(W.validate({ base: 2, wind: spec.wind, clouds: [] }), null);

var src = preset(), edited = W.edit(src, spec);
assert.strictEqual(src.tWindLayers[0].dvSpeed.value, 5, "input untouched");
near(edited.tWindLayers[0].dvAngleRad.value, 270 * Math.PI / 180);
assert.strictEqual(edited.tWindLayers[0].dvSpeed.value, 18);
assert.strictEqual(edited.tWindLayers[2].dvSpeed.value, 25, "extra layer repeats the last");
assert.strictEqual(edited.tWindLayers[2].dvAltitude.value, 10000, "its altitude stays");
assert.strictEqual(edited.tWindLayers[1].gustWaveData.dvSpeedMultiplier.value, 1);
near(edited.oSettings.dvMSLPressure.value, 1020 / 33.8639);
near(edited.oSettings.dvMSLGLTemperature.value, 50);
assert.deepStrictEqual(W.compare(spec, edited), []);
var r = W.reading(edited);
near(r.qnh_hpa, 1020); near(r.temp_c, 10); near(r.wind[1].from_deg, 280);

// compare finds what differs
var off = W.edit(src, spec); off.tWindLayers[0].dvSpeed.value = 20; off.tWindLayers[1].gustWaveData.dvSpeedMultiplier.value = 3;
var bad = W.compare(spec, off);
assert.strictEqual(bad.length, 2); assert.ok(/speed/.test(bad[0]) && /gust/.test(bad[1]));
// 359.8 vs 0.1 deg is 0.3 deg apart
var wrap = { base: 0, wind: [{ alt_ft: 0, from_deg: 359.8, kt: 5 }] };
var p2 = W.edit(src, wrap); p2.tWindLayers[0].dvAngleRad.value = 0.1 * Math.PI / 180;
assert.deepStrictEqual(W.compare(wrap, p2), []);
// too many layers, unknown unit: fail closed
assert.throws(function () { W.edit(src, { base: 0, wind: [1, 2, 3, 4].map(function () { return { alt_ft: 0, from_deg: 0, kt: 0 }; }) }); }, /wind layers/);
var weird = preset(); weird.tWindLayers[0].dvSpeed.unit = "beaufort";
assert.throws(function () { W.edit(weird, spec); }, /unknown speed unit/);

// windAt, compareMeasured
var lay = [{ alt_ft: 0, from_deg: 350, kt: 10 }, { alt_ft: 1000, from_deg: 10, kt: 20 }];
near(W.windAt(lay, 500).kt, 15); near(W.windAt(lay, 500).from_deg, 0); near(W.windAt(lay, -50).kt, 10); near(W.windAt(lay, 9e3).kt, 20);
assert.deepStrictEqual(W.compareMeasured(spec, { wind_dir_deg: 272, wind_kt: 18.4, qnh_hpa: 1020.2, temp_c: 10, alt_ft: 0 }), []);
assert.strictEqual(W.compareMeasured(spec, { wind_dir_deg: 330, wind_kt: 10, qnh_hpa: 1013, temp_c: 20, alt_ft: 0 }).length, 4);
assert.strictEqual(W.compareMeasured(spec, { wind_dir_deg: 280, wind_kt: 25, qnh_hpa: 1020, temp_c: 4.1, alt_ft: 3000 }).length, 0);

// preset lookup
var list = [{ index: 0, name: "LIVE" }, { index: 1, name: "WEATHER_CLEAR_SKY" }, { index: 2, name: "FEW" }];
assert.strictEqual(W.findPreset(list, "CLEAR_SKY"), 1);
assert.strictEqual(W.findPreset(list, "STORM"), -1);

// the whole flow against a fake listener
function fakeListener() {
    var h = {};
    return {
        on: function (n, cb) { h[n] = cb; },
        trigger: function (n) {
            if (n === "ASK_WEATHER_LIST") setTimeout(function () { h.SetWeatherList(list); }, 5);
            if (n === "CUSTOM_WEATHER_PRESET_CHANGED") setTimeout(function () { h.UpdatePreset(JSON.parse(JSON.stringify(src))); }, 5);
        },
        call: function (n, p) {
            assert.strictEqual(n, "UpdateTempWeatherPreset");
            return Promise.resolve(true).then(function (v) { setTimeout(function () { h.UpdatePreset(p); }, 5); return v; });
        }
    };
}
function run(label, sim, expectOk, extra) {
    return new Promise(function (resolve) {
        var s = new W.Setter({ register: fakeListener, simvar: sim, settleMs: 10 });
        s.apply(spec, function (res) {
            assert.strictEqual(res.ok, expectOk, label + " " + JSON.stringify(res));
            if (extra) extra(res);
            resolve();
        });
    });
}
var vars = { "AMBIENT WIND DIRECTION": 271, "AMBIENT WIND VELOCITY": 18, "SEA LEVEL PRESSURE": 1020, "AMBIENT TEMPERATURE": 10, "PLANE ALTITUDE": 0 };
run("good", function (n) { return vars[n]; }, true, function (res) {
    near(res.applied.qnh_hpa, 1020); assert.strictEqual(res.measured.wind_kt, 18); assert.deepStrictEqual(res.mismatches, []);
}).then(function () {
    return run("sim kept old weather", function (n) { return n === "AMBIENT WIND VELOCITY" ? 5 : vars[n]; }, false, function (res) {
        assert.ok(/sim wind/.test(res.mismatches[0]));
    });
}).then(function () {
    var s = new W.Setter({ register: fakeListener, simvar: function () { return 0; }, settleMs: 10 });
    return new Promise(function (resolve) {
        s.apply({ base: "clear", wind: [] }, function (res) { assert.strictEqual(res.ok, false); assert.ok(res.error); resolve(); });
    });
}).then(function () { console.log("foqa-weather: all tests passed"); }, function (e) { console.error(e); process.exit(1); });
