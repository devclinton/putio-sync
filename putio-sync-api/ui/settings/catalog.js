// Every findable setting (feature 061): each section, each card in it, and each field of the
// cards built from a group's `fields`, with the address that opens it. Built from data, not
// the DOM, so the command palette finds settings from any page and Settings search uses the
// same list. No DOM, so it is tested with node; settings/groups.js supplies the groups.

import { SECTIONS, sectionOf, sectionOfKey, settingsHref } from "./sections.js";

/**
 * `groups` are the settings groups ({ id, key, title, fields }) whose id is their card's.
 * Each entry: { section, card, label, detail, hint, href }.
 */
export function buildCatalog(groups) {
  const cardSection = new Map(SECTIONS.flatMap((s) => s.cards.map(([id, title]) => [id, { s, title }])));
  const out = [];
  for (const s of SECTIONS) {
    out.push({ section: s.id, card: null, label: s.title, detail: "Settings", hint: s.help, href: settingsHref(s.id) });
    for (const [id, title] of s.cards) {
      out.push({ section: s.id, card: id, label: title, detail: s.title, hint: "", href: settingsHref(s.id, `card-${id}`) });
    }
  }
  for (const g of groups) {
    // A card SECTIONS does not list is shown in the section that edits its key
    const where = cardSection.get(g.id) || { s: sectionOf(sectionOfKey(g.key)), title: g.title };
    for (const [name, label, hint] of g.fields || []) {
      // form.js gives each input the id set-<key>-<name>; an id is an address of its own
      out.push({ section: where.s.id, card: g.id, label, detail: `${where.s.title} › ${where.title}`, hint: hint || "", href: settingsHref(where.s.id, `set-${g.key}-${name}`) });
    }
  }
  return out;
}

/** The words an entry is matched on */
export const entryText = (e) => `${e.label} ${e.detail}`;
