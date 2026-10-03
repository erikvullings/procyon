import m from "mithril";

export function iconCopy(): m.Vnode {
  return m(
    "svg[width=16][height=16][viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=2][stroke-linecap=round][stroke-linejoin=round]",
    [
      m(
        "path[stroke-linecap=round][stroke-linejoin=round][d=M8 16H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v2m-6 12h8a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-8a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2z]",
      ),
      m(
        "path[stroke-linecap=round][stroke-linejoin=round][d=M16 8v8m-4-5v5m-4-2v2]",
      ),
    ],
  );
}

export function iconSave(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M4 3h13l3 3v15H4z]"),
      m("path[d=M8 3v7h8V3]"),
      m("path[d=M7 21v-8h10v8]"),
    ],
  );
}
