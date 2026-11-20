"use client";

import { AlbumDetail, AlbumEdit, AlbumForm, AlbumList } from "./albums";
import { ArtistDetail, ArtistEdit, ArtistForm, ArtistList } from "./artists";
import type { Kind } from "./common";
import { CompositionDetail, CompositionEdit, CompositionForm, CompositionList } from "./compositions";
import { RecordingDetail, RecordingEdit, RecordingList, RecordingNew } from "./recordings";

export type View = "list" | "new" | "detail" | "edit" | "complete";
type Props = { id: string; query: Record<string, string> };

const VIEWS: Record<Kind, Record<View, (p: Props) => React.ReactNode>> = {
  artists: {
    list: ({ query }) => <ArtistList query={query} />,
    new: () => <ArtistForm />,
    detail: ({ id }) => <ArtistDetail id={id} />,
    edit: ({ id }) => <ArtistEdit id={id} />,
    complete: ({ id }) => <ArtistEdit id={id} fillOnly />,
  },
  compositions: {
    list: ({ query }) => <CompositionList query={query} />,
    new: () => <CompositionForm />,
    detail: ({ id }) => <CompositionDetail id={id} />,
    edit: ({ id }) => <CompositionEdit id={id} />,
    complete: ({ id }) => <CompositionEdit id={id} fillOnly />,
  },
  recordings: {
    list: ({ query }) => <RecordingList query={query} />,
    new: ({ query }) => <RecordingNew query={query} />,
    detail: ({ id }) => <RecordingDetail id={id} />,
    edit: ({ id }) => <RecordingEdit id={id} />,
    complete: ({ id }) => <RecordingDetail id={id} />,
  },
  albums: {
    list: ({ query }) => <AlbumList query={query} />,
    new: () => <AlbumForm />,
    detail: ({ id }) => <AlbumDetail id={id} />,
    edit: ({ id }) => <AlbumEdit id={id} />,
    complete: ({ id }) => <AlbumEdit id={id} fillOnly />,
  },
};

export function CatalogueView({ kind, view, ...props }: { kind: Kind; view: View } & Props) {
  return <>{VIEWS[kind][view](props)}</>;
}
