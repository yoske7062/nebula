import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { setAutoMode } from "../lib/commands";
import { ensureChampions } from "./useChampions";
import type { DownloadedSkin } from "../types";

const AUTO_KEY = "nebula_auto_mode";
const RANDOM_KEY = "nebula_random_skin";

interface ChampSelectEvent {
  connected: boolean;
  inChampSelect: boolean;
  championId: number;
  skinNum: number;
}

interface Options {
  downloads: DownloadedSkin[];
  skinSelection: Record<string, string>;
  customPaths: string[];
  apply: (zipPaths: string[]) => void;
  notify: (message: string) => void;
}

function loadFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

export function useAutoMode(opts: Options) {
  const [auto, setAuto] = useState(() => loadFlag(AUTO_KEY));
  const [random, setRandom] = useState(() => loadFlag(RANDOM_KEY));
  const [connected, setConnected] = useState(false);
  const lastPick = useRef(0);
  const latest = useRef({ ...opts, random });
  latest.current = { ...opts, random };

  useEffect(() => {
    setAutoMode(auto).catch(() => {});
    localStorage.setItem(AUTO_KEY, auto ? "1" : "0");
  }, [auto]);

  useEffect(() => {
    localStorage.setItem(RANDOM_KEY, random ? "1" : "0");
  }, [random]);

  useEffect(() => {
    if (!auto) {
      setConnected(false);
      return;
    }
    lastPick.current = 0;

    const unlisten = listen<ChampSelectEvent>("lcu-champ-select", async ({ payload }) => {
      setConnected(payload.connected);
      if (!payload.inChampSelect) {
        lastPick.current = 0;
        return;
      }
      if (payload.championId === 0 || payload.championId === lastPick.current) return;
      lastPick.current = payload.championId;

      const champ = (await ensureChampions()).find((c) => c.key === String(payload.championId));
      if (!champ) return;

      const { downloads, skinSelection, customPaths, apply, notify, random } = latest.current;
      const owned = downloads.filter((d) => d.champion_name === champ.name);
      const pick = random
        ? owned[Math.floor(Math.random() * owned.length)]
        : owned.find((d) => d.skin_name === skinSelection[champ.name]);
      if (!pick) return;

      if (payload.skinNum !== 0) {
        notify("Pick the default skin in the client or the skin won't show");
      }
      apply([pick.zip_path, ...customPaths]);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [auto]);

  return {
    auto,
    random,
    connected,
    toggleAuto: () => setAuto((v) => !v),
    toggleRandom: () => setRandom((v) => !v),
  };
}
