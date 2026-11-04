import { Children, isValidElement, useEffect, useId, useRef, useState } from "react";
import type { HTMLAttributes, ReactNode } from "react";
import { createPortal } from "react-dom";
import { headingExplanation } from "./headingHelp";

function textOf(children: ReactNode): string {
  return Children.toArray(children).map(child => {
    if (typeof child === "string" || typeof child === "number") return String(child);
    if (isValidElement<{children?: ReactNode}>(child)) return child.type === "br" ? " " : textOf(child.props.children);
    return "";
  }).join("").replace(/\s+/g, " ").trim();
}

export function HelpHeading({as: Tag="h2", topic="", help, children, openOnFocus=true, ...props}: HTMLAttributes<HTMLElement> & {
  as?: "h1"|"h2"|"h3"|"h4"|"h5"|"h6"|"legend"|"strong";
  topic?: string;
  help?: string;
  openOnFocus?: boolean;
}) {
  const label = textOf(children);
  const description = help ?? headingExplanation(label, topic);
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [position, setPosition] = useState({left:12, top:12});
  const cancelClose = () => clearTimeout(closeTimer.current);
  const close = () => { cancelClose(); setOpen(false); setPinned(false); };
  const show = () => { cancelClose(); window.dispatchEvent(new CustomEvent("heading-help-open", {detail:id})); setOpen(true); };
  const leave = () => { if (!pinned && document.activeElement !== trigger.current) closeTimer.current=setTimeout(close,150); };
  useEffect(() => () => clearTimeout(closeTimer.current), []);
  useEffect(() => { close(); }, [label, description]);
  useEffect(() => { if (open && trigger.current?.closest("[hidden]")) close(); });
  useEffect(() => {
    if (!open) return;
    const place = () => {
      const rect=trigger.current?.getBoundingClientRect();
      const tip=bubble.current?.getBoundingClientRect();
      if (!rect || !tip) return;
      setPosition({left:Math.max(12,Math.min(rect.left,window.innerWidth-tip.width-12)),
        top:Math.max(12,rect.bottom+8+tip.height<=window.innerHeight-12?rect.bottom+8:rect.top-tip.height-8)});
    };
    const outside = (event: PointerEvent) => { if (!trigger.current?.contains(event.target as Node) && !bubble.current?.contains(event.target as Node)) close(); };
    const other = (event: Event) => { if ((event as CustomEvent).detail!==id) close(); };
    const escape = (event: KeyboardEvent) => { if (event.key==="Escape") {event.preventDefault();event.stopImmediatePropagation();close();} };
    place();
    window.addEventListener("resize",place);
    document.addEventListener("scroll",place,true);
    document.addEventListener("pointerdown",outside,true);
    document.addEventListener("keydown",escape,true);
    window.addEventListener("heading-help-open",other);
    return () => {window.removeEventListener("resize",place);document.removeEventListener("scroll",place,true);document.removeEventListener("pointerdown",outside,true);document.removeEventListener("keydown",escape,true);window.removeEventListener("heading-help-open",other);};
  }, [open,id]);
  return <Tag {...props} aria-label={props["aria-label"] ?? label}>
    {children}<button type="button" className="heading-help" ref={trigger} aria-label={`解释：${label}`}
      aria-expanded={open} aria-describedby={open?id:undefined}
      onPointerDown={event=>event.stopPropagation()} onMouseEnter={show} onMouseLeave={leave}
      onFocus={openOnFocus ? show : undefined} onBlur={close}
      onKeyDown={event=>{if(event.key==="Enter"||event.key===" ")event.stopPropagation();}}
      onClick={event=>{event.preventDefault();event.stopPropagation();if(pinned)close();else{show();setPinned(true);}}}>?</button>
    {open&&createPortal(<div id={id} role="tooltip" className="heading-help-tip" ref={bubble} style={position}
      onMouseEnter={cancelClose} onMouseLeave={leave} onClick={event=>event.stopPropagation()}>
      <strong>{label}</strong><p>{description}</p>
    </div>,trigger.current?.closest("dialog, [role=dialog]")??document.body)}
  </Tag>;
}
