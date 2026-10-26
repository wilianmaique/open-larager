import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { Check, ChevronDown, ChevronUp } from 'lucide-react';
import { useApp } from './state';

interface SelectOption<Value extends string> {
  value: Value;
  label: string;
  description?: string;
  disabled?: boolean;
}

type SelectProps<Value extends string> = Pick<
  ButtonHTMLAttributes<HTMLButtonElement>,
  'id' | 'aria-label' | 'aria-labelledby' | 'aria-describedby' | 'disabled' | 'className'
> & {
  value: Value;
  options: readonly SelectOption<Value>[];
  onValueChange: (value: Value) => void;
  placeholder?: string;
};

export function Select<Value extends string>({
  value,
  options,
  onValueChange,
  placeholder,
  className = '',
  disabled = false,
  ...props
}: SelectProps<Value>) {
  const generatedId = useId();
  const triggerId = props.id ?? `select-${generatedId}`;
  const listId = `${triggerId}-options`;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef({ text: '', time: 0 });
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const enabledIndices = options.flatMap((option, index) => (option.disabled ? [] : [index]));
  const unavailable = disabled || !enabledIndices.length;

  const close = () => {
    listRef.current?.hidePopover();
    setOpen(false);
    searchRef.current.text = '';
  };
  const show = (index = selectedIndex) => {
    if (unavailable) return;
    setActiveIndex(enabledIndices.includes(index) ? index : enabledIndices[0]);
    listRef.current?.showPopover();
    setOpen(true);
  };
  const choose = (index: number) => {
    const option = options[index];
    if (!option || option.disabled || unavailable) return;
    onValueChange(option.value);
    close();
    triggerRef.current?.focus({ preventScroll: true });
  };

  useLayoutEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current!;
    const list = listRef.current!;
    const position = () => {
      const bounds = trigger.getBoundingClientRect();
      const viewport = window.visualViewport;
      const viewportTop = viewport?.offsetTop ?? 0;
      const viewportLeft = viewport?.offsetLeft ?? 0;
      const viewportHeight = viewport?.height ?? window.innerHeight;
      const viewportWidth = viewport?.width ?? window.innerWidth;
      const width = Math.min(bounds.width, viewportWidth - 24);
      list.style.width = `${width}px`;
      const desiredHeight = Math.min(list.scrollHeight + 2, 280);
      const below = viewportTop + viewportHeight - bounds.bottom - 14;
      const above = bounds.top - viewportTop - 14;
      const upwards = below < desiredHeight && above > below;
      const height = Math.max(0, Math.min(desiredHeight, upwards ? above : below));
      list.style.maxHeight = `${height}px`;
      list.style.left = `${Math.max(viewportLeft + 12, Math.min(bounds.left, viewportLeft + viewportWidth - width - 12))}px`;
      list.style.top = `${upwards ? bounds.top - height - 6 : bounds.bottom + 6}px`;
      list.dataset.side = upwards ? 'top' : 'bottom';
    };
    position();
    const onScroll = (event: Event) => {
      if (!list.contains(event.target as Node)) position();
    };
    const observer = new ResizeObserver(position);
    observer.observe(trigger);
    window.addEventListener('resize', position);
    window.addEventListener('scroll', onScroll, true);
    window.visualViewport?.addEventListener('resize', position);
    window.visualViewport?.addEventListener('scroll', position);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', position);
      window.removeEventListener('scroll', onScroll, true);
      window.visualViewport?.removeEventListener('resize', position);
      window.visualViewport?.removeEventListener('scroll', position);
    };
  }, [open, options.length]);

  useEffect(() => {
    if (unavailable) close();
  }, [unavailable]);

  useEffect(() => {
    if (open && activeIndex >= 0)
      document.getElementById(`${listId}-${activeIndex}`)?.scrollIntoView({ block: 'nearest' });
  }, [open, activeIndex, listId]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.nativeEvent.isComposing || unavailable) return;
    if (Date.now() - searchRef.current.time >= 700) searchRef.current.text = '';
    if (event.key === 'Escape' && open) {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === 'Tab') {
      close();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      searchRef.current.text = '';
      const direction = event.key === 'ArrowDown' ? 1 : -1;
      if (!open)
        show(
          selectedIndex >= 0
            ? selectedIndex
            : direction > 0
              ? enabledIndices[0]
              : enabledIndices.at(-1),
        );
      else {
        const index = enabledIndices.indexOf(activeIndex);
        setActiveIndex(
          enabledIndices[(index + direction + enabledIndices.length) % enabledIndices.length],
        );
      }
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      searchRef.current.text = '';
      const index = event.key === 'Home' ? enabledIndices[0] : enabledIndices.at(-1)!;
      if (open) setActiveIndex(index);
      else show(index);
    } else if (event.key === 'Enter' || (event.key === ' ' && !searchRef.current.text)) {
      event.preventDefault();
      if (open) choose(activeIndex);
      else show();
    } else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      const now = Date.now();
      const text = (now - searchRef.current.time < 700 ? searchRef.current.text : '') + event.key;
      searchRef.current = { text, time: now };
      const normalize = (label: string) =>
        label.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();
      const query = normalize([...text].every((letter) => letter === text[0]) ? text[0] : text);
      const start = open ? activeIndex : selectedIndex;
      const ordered = [
        ...enabledIndices.filter((index) => index > start),
        ...enabledIndices.filter((index) => index <= start),
      ];
      const match = (query.length > 1 ? [start, ...ordered] : ordered).find(
        (index) =>
          options[index] &&
          !options[index].disabled &&
          normalize(options[index].label).startsWith(query),
      );
      if (match !== undefined) {
        if (open) setActiveIndex(match);
        else show(match);
      }
    }
  };

  return (
    <div className={`select-control ${className}`}>
      <button
        {...props}
        id={triggerId}
        ref={triggerRef}
        type="button"
        role="combobox"
        className="select-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={open && activeIndex >= 0 ? `${listId}-${activeIndex}` : undefined}
        disabled={unavailable}
        onClick={() => (open ? close() : show())}
        onKeyDown={onKeyDown}
        onBlur={close}
      >
        <span className={`select-value ${selectedIndex < 0 ? 'select-placeholder' : ''}`}>
          {options[selectedIndex]?.label ?? placeholder}
        </span>
        <ChevronDown size={15} className="select-chevron" aria-hidden="true" />
      </button>
      <div
        ref={listRef}
        id={listId}
        role="listbox"
        aria-label={props['aria-label']}
        aria-labelledby={props['aria-label'] ? undefined : (props['aria-labelledby'] ?? triggerId)}
        className="select-popover"
        popover="auto"
        onToggle={(event) => {
          const visible = event.currentTarget.matches(':popover-open');
          setOpen(visible);
          if (!visible) searchRef.current.text = '';
        }}
        onPointerDown={(event) => event.preventDefault()}
      >
        {options.map((option, index) => (
          <div
            id={`${listId}-${index}`}
            key={option.value}
            role="option"
            aria-selected={option.value === value}
            aria-disabled={option.disabled || undefined}
            data-active={activeIndex === index}
            className="select-option"
            onPointerMove={() => !option.disabled && setActiveIndex(index)}
            onClick={() => choose(index)}
          >
            <span className="select-option-label">{option.label}</span>
            {option.description && (
              <span className="select-option-detail">{option.description}</span>
            )}
            <Check size={14} className="select-option-check" aria-hidden="true" />
          </div>
        ))}
      </div>
    </div>
  );
}

type ChoiceProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'type' | 'className' | 'children'
> & {
  children?: ReactNode;
  className?: string;
};

export function Checkbox({ children, className = '', ...props }: ChoiceProps) {
  return (
    <label className={`checkbox choice-control ${className}`}>
      <input {...props} type="checkbox" className="control-input" />
      <span className="checkbox-indicator" aria-hidden="true">
        <Check size={12} strokeWidth={2.5} />
      </span>
      <span>{children}</span>
    </label>
  );
}

export function Switch({ className = '', ...props }: Omit<ChoiceProps, 'children'>) {
  return (
    <label className={`switch choice-control ${className}`}>
      <input {...props} type="checkbox" role="switch" className="control-input" />
      <span className="switch-track" aria-hidden="true" />
    </label>
  );
}

export function RadioCard({ children, className = '', ...props }: ChoiceProps) {
  return (
    <label className={`radio-card choice-control ${props.checked ? 'selected' : ''} ${className}`}>
      <input {...props} type="radio" className="control-input" />
      <span className="radio-indicator" aria-hidden="true" />
      {children}
    </label>
  );
}

type NumberInputProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'type' | 'onChange' | 'value' | 'min' | 'max' | 'step'
> & {
  label: string;
  value: string | number;
  min?: number;
  max?: number;
  step?: number;
  onValueChange: (value: string) => void;
};

export function NumberInput({
  label,
  value,
  onValueChange,
  min,
  max,
  step = 1,
  className = '',
  ...props
}: NumberInputProps) {
  const { t } = useApp();
  const ref = useRef<HTMLInputElement>(null);
  const change = (direction: number) => {
    const input = ref.current!;
    input.stepUp(direction);
    onValueChange(input.value);
    input.focus({ preventScroll: true });
  };
  return (
    <span className={`number-input ${className}`}>
      <input
        {...props}
        ref={ref}
        type="number"
        aria-label={label}
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(event) => onValueChange(event.target.value)}
      />
      <span className="number-steppers">
        <button
          type="button"
          aria-label={t('increaseValue', { name: label })}
          disabled={
            props.disabled ||
            props.readOnly ||
            (max !== undefined && value !== '' && Number(value) >= max)
          }
          onClick={() => change(1)}
        >
          <ChevronUp size={11} aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label={t('decreaseValue', { name: label })}
          disabled={
            props.disabled ||
            props.readOnly ||
            (min !== undefined && value !== '' && Number(value) <= min)
          }
          onClick={() => change(-1)}
        >
          <ChevronDown size={11} aria-hidden="true" />
        </button>
      </span>
    </span>
  );
}
