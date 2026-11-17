// The settings catalog for this UI (feature 061): every settings module's field groups, for
// the command palette and Settings search. catalog.js does the work and is tested with node.

import { groups as advanced } from "./advanced.js";
import { groups as downloads } from "./downloads.js";
import { groups as filters } from "./filters.js";
import { groups as general } from "./general.js";
import { groups as integrations } from "./integrations.js";
import { groups as media } from "./media.js";
import { groups as plugins } from "./plugins.js";
import { groups as putio } from "./putio.js";
import { groups as subtitles } from "./subtitles.js";
import { groups as translation } from "./translation.js";
import { buildCatalog } from "./catalog.js";

let built = null;

/** Every findable setting, built once on first use */
export const settingsCatalog = () => (built ||= buildCatalog([...general, ...putio, ...downloads, ...subtitles, ...translation, ...filters, ...integrations, ...plugins, ...media, ...advanced]));
