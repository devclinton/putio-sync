// Media server section (feature 022): share downloaded files, and optionally a Put.io folder,
// with TVs and players on the home network.

import { mediaServerExtra } from "../media_server.js";
import { groupCard } from "./form.js";

const MEDIA = {
  id: "media-server",
  key: "media-server",
  title: "Media server",
  fields: [
    ["enabled", "Media server", "Show your files to TVs and players on the home network (DLNA / UPnP)", "b"],
    ["name", "Name", "What players show; up to 64 characters", "s"],
    ["port", "Port", "TCP port for browsing and streaming; not 1900, and not the API's or metrics' port", "n", 1, 65535],
    ["bind", "Listen on", "One per line: ip:port or interface:port; only the address is used. Empty is every LAN address", "l"],
  ],
  advanced: ["port", "bind"],
  extra: mediaServerExtra,
};

export const cards = (ctx) => [groupCard(MEDIA, ctx)];
