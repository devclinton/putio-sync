// Appearance (feature 012): a classic script in <head>, so the saved mode and accent are on
// <html> before the first paint. Pages use window.appearance; nothing reaches the daemon.
(function () {
  var KEY = "putio-sync.appearance";
  var MODES = ["system", "light", "dark"];
  var ACCENTS = ["blue", "indigo", "teal", "green", "orange", "pink", "graphite"];
  var root = document.documentElement;
  var current = { mode: "system", accent: "blue" };

  function clean(v) {
    v = v || {};
    return {
      mode: MODES.indexOf(v.mode) >= 0 ? v.mode : "system",
      accent: ACCENTS.indexOf(v.accent) >= 0 ? v.accent : "blue",
    };
  }

  function apply() {
    if (current.mode === "system") root.removeAttribute("data-mode");
    else root.setAttribute("data-mode", current.mode);
    if (current.accent === "blue") root.removeAttribute("data-accent");
    else root.setAttribute("data-accent", current.accent);
  }

  try {
    current = clean(JSON.parse(window.localStorage.getItem(KEY)));
  } catch (e) {
    // storage blocked or unreadable: defaults
  }
  apply();

  window.appearance = {
    MODES: MODES,
    ACCENTS: ACCENTS,
    get: function () {
      return { mode: current.mode, accent: current.accent };
    },
    set: function (change) {
      current = clean(Object.assign({}, current, change));
      apply();
      try {
        window.localStorage.setItem(KEY, JSON.stringify(current));
      } catch (e) {
        // not remembered, still applied
      }
      document.dispatchEvent(new CustomEvent("appearancechange", { detail: window.appearance.get() }));
    },
  };
})();
