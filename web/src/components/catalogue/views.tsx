"use client";

import { AlbumDetail, AlbumEdit, AlbumForm, AlbumList } from "./albums";
import { ArtistDetail, ArtistEdit, ArtistForm, ArtistList } from "./artists";
import type { Kind } from "./common";
import { CompositionDetail, CompositionEdit, CompositionForm, CompositionList } from "./compositions";
import { RecordingDetail, RecordingEdit, RecordingList, RecordingNew } from "./recordings";

export type View = "list" | "new" | "detail" | "edit";
type Props = { id: string; query: Record<string, string> };

const VIEWS: Record<Kind, Record<View, (p: Props) => React.ReactNode>> = {
  artists: {
    list: ({ query }) => <ArtistList query={query} />,
    new: () => <ArtistForm />,
    detail: ({ id }) => <ArtistDetail id={id} />,
    edit: ({ id }) => <ArtistEdit id={id} />,
  },
  compositions: {
    list: ({ query }) => <CompositionList query={query} />,
    new: () => <CompositionForm />,
    detail: ({ id }) => <CompositionDetail id={id} />,
    edit: ({ id }) => <CompositionEdit id={id} />,
  },
  recordings: {
    list: ({ query }) => <RecordingList query={query} />,
    new: ({ query }) => <RecordingNew query={query} />,
    detail: ({ id }) => <RecordingDetail id={id} />,
    edit: ({ id }) => <RecordingEdit id={id} />,
  },
  albums: {
    list: ({ query }) => <AlbumList query={query} />,
    new: () => <AlbumForm />,
    detail: ({ id }) => <AlbumDetail id={id} />,
    edit: ({ id }) => <AlbumEdit id={id} />,
  },
};

export function CatalogueView({ kind, view, ...props }: { kind: Kind; view: View } & Props) {
  return <>{VIEWS[kind][view](props)}</>;
}
