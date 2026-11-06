import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";

/**
 * A small window for typing a question the recognizer missed or got wrong.
 * The panel itself never takes focus, so typing happens here; the window
 * closes once the question is sent.
 */
export function AskWindow() {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => input.current?.focus(), []);
  const send = async () => {
    const q = text.trim();
    if (!q || sending) return;
    setSending(true);
    try {
      await backend().invoke("user_action", { action: { type: "ask", text: q } });
      setText("");
      await backend().closeWindow();
    } finally {
      setSending(false);
    }
  };
  return (
    <div className="app ask">
      <div className="app-body">
        <label htmlFor="ask-text">
          <strong>{t("ask.title")}</strong>
        </label>
        <textarea
          id="ask-text"
          ref={input}
          rows={3}
          style={{ width: "100%", marginTop: 6 }}
          placeholder={t("ask.placeholder")}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
            if (e.key === "Escape") void backend().closeWindow();
          }}
        />
        <div className="row" style={{ marginTop: 8 }}>
          <span className="muted" style={{ fontSize: 12 }}>
            {t("ask.hint")}
          </span>
          <span className="spacer" />
          <button className="btn" onClick={() => void backend().closeWindow()}>
            {t("common.cancel")}
          </button>
          <button className="btn btn-primary" disabled={!text.trim() || sending} onClick={() => void send()}>
            {t("ask.send")}
          </button>
        </div>
      </div>
    </div>
  );
}
