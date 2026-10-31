import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ModelSlot } from "../backend/types";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { PriceTable } from "../bindings/PriceTable";
import { price, usd } from "../util/format";
import { audioPerHour, filterOptions, tokensPerMillion, type ModelOption } from "../util/models";

let cached: PriceTable | null = null;

/** MeetCue's price list (built in, refreshed from the providers' pages at start-up). */
export function usePriceTable(): PriceTable | null {
  const [table, setTable] = useState<PriceTable | null>(cached);
  useEffect(() => {
    let alive = true;
    void backend()
      .invoke("get_pricing")
      .then((t) => {
        cached = t;
        if (alive) setTable(t);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  return table;
}

/** "$0.12 / audio hour" or "$1 in · $5 out / 1M tokens". */
export function useUnitPrice(): (p: PriceEntry) => string | null {
  const { t } = useTranslation();
  return (p) => {
    const h = audioPerHour(p);
    if (h !== null) return t("models.perAudioHour", { price: price(h) });
    const m = tokensPerMillion(p);
    if (m) return t("models.perMillion", { input: price(m.input), output: price(m.output) });
    return null;
  };
}

export type ListState = "loading" | "needsKey" | "loaded" | "none";

/**
 * Model field with a list that opens under it: every model with a known
 * price, plus those the provider reports for the account, each with its price
 * and the estimated cost of a meeting hour. Any other name can still be typed.
 */
export function ModelCombo({
  id,
  slot,
  value,
  options,
  listState,
  providerName,
  disabled,
  placeholder,
  onChange,
}: {
  id: string;
  slot: ModelSlot;
  value: string;
  options: ModelOption[];
  listState: ListState;
  providerName: string;
  disabled: boolean;
  placeholder: string;
  onChange: (model: string | null) => void;
}) {
  const { t } = useTranslation();
  const unitPrice = useUnitPrice();
  const listId = useId();
  const [open, setOpen] = useState(false);
  // What the user typed since opening; empty shows the whole list.
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  const shown = useMemo(() => filterOptions(options, query ?? ""), [options, query]);
  const typed = (query ?? "").trim();
  const custom = typed && !options.some((o) => o.id === typed) ? typed : null;
  // Rows: the typed name first (when it isn't listed), then the matches.
  const all: ({ kind: "custom"; id: string } | { kind: "option"; option: ModelOption })[] = [
    ...(custom ? [{ kind: "custom" as const, id: custom }] : []),
    ...shown.map((option) => ({ kind: "option" as const, option })),
  ];
  const cheapest = useMemo(() => {
    const priced = options.filter((o) => o.perHour !== null);
    if (priced.length < 2) return null;
    return priced.reduce((a, b) => ((b.perHour ?? Infinity) < (a.perHour ?? Infinity) ? b : a)).id;
  }, [options]);

  useEffect(() => {
    if (!open) return;
    const i = all.findIndex((r) => r.kind === "option" && r.option.id === value);
    setActive(query === null && i >= 0 ? i : 0);
    // Only when the list opens or the filter changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, query]);

  useEffect(() => {
    if (!open) return;
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [active, open]);

  const close = () => {
    setOpen(false);
    setQuery(null);
  };

  const choose = (model: string) => {
    onChange(model.trim() || null);
    close();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const n = all.length;
      if (!n) return;
      setActive((a) => (e.key === "ArrowDown" ? (a + 1) % n : (a - 1 + n) % n));
    } else if (e.key === "Enter") {
      if (!open) return;
      e.preventDefault();
      const row = all[active];
      if (row) choose(row.kind === "custom" ? row.id : row.option.id);
      else if (typed) choose(typed);
      else close();
    } else if (e.key === "Escape") {
      if (open) {
        e.preventDefault();
        close();
      }
    } else if (e.key === "Tab") {
      close();
    }
  };

  const status =
    listState === "loading"
      ? t("models.loading", { provider: providerName })
      : listState === "needsKey"
        ? t("models.needsKey", { provider: providerName })
        : null;

  return (
    <div className={`combo${open ? " combo-open" : ""}`}>
      <div className="combo-input">
        <input
          ref={input}
          id={id}
          type="text"
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && all[active] ? `${listId}-${active}` : undefined}
          autoComplete="off"
          spellCheck={false}
          disabled={disabled}
          placeholder={placeholder}
          value={query ?? value}
          onFocus={() => setOpen(true)}
          onClick={() => setOpen(true)}
          onChange={(e) => {
            // A typed name counts right away; the list only narrows to it.
            setQuery(e.target.value);
            setOpen(true);
            onChange(e.target.value.trim() || null);
          }}
          onBlur={close}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          className="combo-toggle"
          tabIndex={-1}
          disabled={disabled}
          aria-label={t("models.showList")}
          onMouseDown={(e) => {
            e.preventDefault();
            if (open) close();
            else {
              setOpen(true);
              input.current?.focus();
            }
          }}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M2.5 4.5 6 8l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </div>
      {open && !disabled && (
        <div className="combo-pop">
          <ul ref={list} id={listId} role="listbox" aria-label={t("models.listLabel", { provider: providerName })}>
            {all.map((row, i) => {
              const rowId = `${listId}-${i}`;
              const props = {
                id: rowId,
                role: "option" as const,
                "data-index": i,
                "data-active": i === active || undefined,
                onMouseEnter: () => setActive(i),
                // Keep focus in the field so the blur doesn't commit the typed text first.
                onMouseDown: (e: MouseEvent) => e.preventDefault(),
              };
              if (row.kind === "custom") {
                return (
                  <li key="custom" {...props} aria-selected={false} className="combo-row combo-custom" onClick={() => choose(row.id)}>
                    <span className="combo-main">
                      <span className="combo-name">{t("models.useTyped", { model: row.id })}</span>
                      <span className="combo-id muted">{t("models.useTypedHint", { provider: providerName })}</span>
                    </span>
                  </li>
                );
              }
              const o = row.option;
              const selected = o.id === value;
              const unit = o.price ? unitPrice(o.price) : null;
              return (
                <li key={o.id} {...props} aria-selected={selected} className="combo-row" onClick={() => choose(o.id)}>
                  <span className="combo-check" aria-hidden="true">
                    {selected ? "✓" : ""}
                  </span>
                  <span className="combo-main">
                    <span className="combo-name">
                      {o.name}
                      {o.id === cheapest && <span className="badge badge-ok">{t("models.cheapest")}</span>}
                      {o.onAccount && listState === "loaded" && <span className="badge">{t("models.onAccount")}</span>}
                    </span>
                    {o.name !== o.id && <span className="combo-id mono">{o.id}</span>}
                  </span>
                  <span className="combo-price">
                    {o.perHour !== null ? (
                      <>
                        <span className="combo-hour">{t("models.perMeetingHour", { price: usd(o.perHour) })}</span>
                        {unit && <span className="combo-unit muted">{unit}</span>}
                      </>
                    ) : (
                      <span className="combo-unit muted">{t("models.noPrice")}</span>
                    )}
                  </span>
                </li>
              );
            })}
            {!all.length && <li className="combo-empty muted">{t("models.empty", { provider: providerName })}</li>}
          </ul>
          <div className="combo-foot muted">
            {status ?? (slot === "asr" ? t("models.footAsr") : t("models.footLlm"))}
          </div>
        </div>
      )}
    </div>
  );
}
