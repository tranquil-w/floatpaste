import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useWorkbenchStore } from "./store";
import { WORKBENCH_SESSION_START_EVENT, WORKBENCH_SESSION_END_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";

export function WorkbenchShell() {
  const { setSession, setMode, setSelectedItemId, setKeyword } = useWorkbenchStore();

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let offStart: (() => void) | undefined;
    let offEnd: (() => void) | undefined;

    void listen<{
      source: string;
      itemId?: string;
      initialKeyword?: string;
    }>(WORKBENCH_SESSION_START_EVENT, (event) => {
      const { source, itemId, initialKeyword } = event.payload;
      setSession({
        source: source as "picker_edit" | "picker_search" | "global",
        initialItemId: itemId,
        initialKeyword,
      });
      if (itemId) {
        setMode("edit");
        setSelectedItemId(itemId);
      } else {
        setMode("search");
        setSelectedItemId(null);
      }
      setKeyword(initialKeyword ?? "");
    }).then((cleanup) => { offStart = cleanup; });

    void listen(WORKBENCH_SESSION_END_EVENT, () => {
      setSession(null);
      setMode("search");
      setSelectedItemId(null);
      setKeyword("");
    }).then((cleanup) => { offEnd = cleanup; });

    return () => {
      offStart?.();
      offEnd?.();
    };
  }, [setSession, setMode, setSelectedItemId, setKeyword]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[color:var(--cp-window-shell)] text-ink">
      <div className="flex h-full flex-col">
        <header className="shrink-0 border-b border-[color:var(--cp-border-weak)] px-4 py-3">
          <TopBar />
        </header>
        <main className="flex min-h-0 flex-1">
          <aside className="w-80 shrink-0 border-r border-[color:var(--cp-border-weak)]">
            <ResultList />
          </aside>
          <section className="min-w-0 flex-1">
            <EditPanel />
          </section>
        </main>
      </div>
    </div>
  );
}

function TopBar() {
  const { session, keyword, setKeyword } = useWorkbenchStore();
  return (
    <div className="flex items-center gap-4">
      <span className="text-xs text-[color:var(--cp-text-muted)]">
        {session?.source === "picker_edit" && "从 Picker 编辑"}
        {session?.source === "picker_search" && "从 Picker 搜索"}
        {session?.source === "global" && "全局搜索"}
      </span>
      <input
        className="flex-1 rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] px-3 py-2 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
        placeholder="搜索记录..."
        value={keyword}
        onChange={(e) => setKeyword(e.target.value)}
      />
    </div>
  );
}

function ResultList() {
  return (
    <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
      结果列表（待实现）
    </div>
  );
}

function EditPanel() {
  return (
    <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
      编辑面板（待实现）
    </div>
  );
}
