// Language picker for the docs (feature 054): a <select> in the menu bar that
// opens the same page (and heading: translated headings keep their English
// ids) in another language. The English book is at docs/, each
// translation at docs/<lang>/, and build.sh lists them in docs/languages.json.
(function () {
    "use strict";
    var bar = document.querySelector(".right-buttons");
    if (!bar || typeof path_to_root === "undefined") return;
    var lang = document.documentElement.lang || "en";
    var book = new URL(path_to_root || "./", location.href);
    var docs = new URL(lang === "en" ? "./" : "../", book);
    var page = location.href.slice(book.href.length).split("#")[0];

    fetch(new URL("languages.json", docs)).then(function (r) {
        return r.ok ? r.json() : [];
    }).then(function (langs) {
        if (langs.length < 2) return;
        var select = document.createElement("select");
        select.className = "lang-picker";
        select.setAttribute("aria-label", "Language");
        langs.forEach(function (code) {
            var name = code;
            try { name = new Intl.DisplayNames([code], { type: "language" }).of(code); } catch (e) {}
            var option = new Option(name, code, false, code === lang);
            option.lang = code;
            select.add(option);
        });
        select.addEventListener("change", function () {
            location.href = new URL((select.value === "en" ? "" : select.value + "/") + page + location.hash, docs).href;
        });
        bar.insertBefore(select, bar.firstChild);
    }).catch(function () {});
})();
