import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import { isTauriRuntime } from "../../bridge/runtime";
import { SETTINGS_SECTIONS, type SettingsSectionId } from "./settingsSections";

const SIDEBAR_BREAKPOINT = 880;
const SCROLL_OFFSET = 96;
const PROGRAMMATIC_SCROLL_UNLOCK_DELAY = 120;

type LayoutMode = "sidebar" | "compact";

type UseSettingsNavigationResult = {
  layoutMode: LayoutMode;
  activeSectionId: SettingsSectionId;
  registerContainer: (element: HTMLElement | null) => void;
  registerSection: (id: SettingsSectionId, element: HTMLElement | null) => void;
  scrollToSection: (id: SettingsSectionId) => void;
};

function getLayoutMode(width: number): LayoutMode {
  if (isTauriRuntime()) {
    return "sidebar";
  }

  return width >= SIDEBAR_BREAKPOINT ? "sidebar" : "compact";
}

function getActiveSectionId(sectionElements: Map<SettingsSectionId, HTMLElement>) {
  const firstSectionId = SETTINGS_SECTIONS[0]?.id ?? "shortcuts";
  let candidateId: SettingsSectionId | null = null;
  let candidateDistance = Number.POSITIVE_INFINITY;

  for (const section of SETTINGS_SECTIONS) {
    const element = sectionElements.get(section.id);
    if (!element) continue;

    const top = element.getBoundingClientRect().top;
    if (top <= SCROLL_OFFSET) {
      const distance = Math.abs(SCROLL_OFFSET - top);
      if (distance < candidateDistance) {
        candidateDistance = distance;
        candidateId = section.id;
      }
    }
  }

  if (candidateId) {
    return candidateId;
  }

  return firstSectionId;
}

export function useSettingsNavigation(): UseSettingsNavigationResult {
  const [layoutMode, setLayoutMode] = useState<LayoutMode>(() => {
    if (typeof window === "undefined") {
      return "sidebar";
    }

    return getLayoutMode(window.innerWidth);
  });
  const [activeSectionId, setActiveSectionId] = useState<SettingsSectionId>(
    SETTINGS_SECTIONS[0]?.id ?? "shortcuts",
  );
  const [containerElement, setContainerElement] = useState<HTMLElement | null>(null);

  const sectionElementsRef = useRef(new Map<SettingsSectionId, HTMLElement>());
  const programmaticScrollTargetRef = useRef<SettingsSectionId | null>(null);
  const unlockTimerRef = useRef<number | null>(null);

  const syncLayoutMode = useEffectEvent((nextWidth?: number) => {
    const width =
      nextWidth ??
      containerElement?.getBoundingClientRect().width ??
      window.innerWidth;
    setLayoutMode(getLayoutMode(width));
  });

  const syncActiveSection = useEffectEvent(() => {
    if (programmaticScrollTargetRef.current) {
      if (unlockTimerRef.current !== null) {
        window.clearTimeout(unlockTimerRef.current);
      }

      unlockTimerRef.current = window.setTimeout(() => {
        programmaticScrollTargetRef.current = null;
        unlockTimerRef.current = null;
        setActiveSectionId(getActiveSectionId(sectionElementsRef.current));
      }, PROGRAMMATIC_SCROLL_UNLOCK_DELAY);
      return;
    }

    setActiveSectionId(getActiveSectionId(sectionElementsRef.current));
  });

  useEffect(() => {
    syncLayoutMode();
    syncActiveSection();

    const handleScroll = () => {
      syncActiveSection();
    };
    window.addEventListener("scroll", handleScroll, { passive: true });

    let observer: ResizeObserver | null = null;
    let handleResize: (() => void) | undefined;

    if (containerElement && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver((entries) => {
        const entry = entries[0];
        syncLayoutMode(entry?.contentRect.width);
        syncActiveSection();
      });
      observer.observe(containerElement);
    } else {
      handleResize = () => {
        syncLayoutMode();
        syncActiveSection();
      };
      window.addEventListener("resize", handleResize);
    }

    return () => {
      window.removeEventListener("scroll", handleScroll);
      if (handleResize) {
        window.removeEventListener("resize", handleResize);
      }
      observer?.disconnect();
      if (unlockTimerRef.current !== null) {
        window.clearTimeout(unlockTimerRef.current);
      }
    };
  }, [containerElement, syncActiveSection, syncLayoutMode]);

  const registerContainer = useCallback((element: HTMLElement | null) => {
    setContainerElement((current) => (current === element ? current : element));
  }, []);

  const registerSection = useCallback((id: SettingsSectionId, element: HTMLElement | null) => {
    const current = sectionElementsRef.current.get(id) ?? null;
    if (current === element) {
      return;
    }

    if (element) {
      sectionElementsRef.current.set(id, element);
    } else {
      sectionElementsRef.current.delete(id);
    }

    if (!programmaticScrollTargetRef.current) {
      setActiveSectionId(getActiveSectionId(sectionElementsRef.current));
    }
  }, []);

  const scrollToSection = (id: SettingsSectionId) => {
    const element = sectionElementsRef.current.get(id);
    if (!element) {
      return;
    }

    programmaticScrollTargetRef.current = id;
    setActiveSectionId(id);

    if (unlockTimerRef.current !== null) {
      window.clearTimeout(unlockTimerRef.current);
      unlockTimerRef.current = null;
    }

    const targetTop = window.scrollY + element.getBoundingClientRect().top - SCROLL_OFFSET;
    window.scrollTo({
      top: Math.max(targetTop, 0),
      behavior: "smooth",
    });
  };

  return {
    layoutMode,
    activeSectionId,
    registerContainer,
    registerSection,
    scrollToSection,
  };
}
