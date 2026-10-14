// @vitest-environment jsdom
import { defineComponent, nextTick, ref, toRef } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  driverConfig: undefined as Record<string, unknown> | undefined,
  focusHandler: undefined as ((event: { payload: boolean }) => void) | undefined,
  isFocused: vi.fn(),
  isVisible: vi.fn(),
  driver: {
    destroy: vi.fn(),
    drive: vi.fn(),
    moveTo: vi.fn(),
  },
}));

vi.mock('driver.js', () => ({
  driver: vi.fn((config) => {
    mocks.driverConfig = config;
    return mocks.driver;
  }),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    isFocused: mocks.isFocused,
    isVisible: mocks.isVisible,
    onFocusChanged: vi.fn(async (handler) => {
      mocks.focusHandler = handler;
      return vi.fn();
    }),
  }),
}));

import {
  isOnboardingCompleted,
  markOnboardingCompleted,
  ONBOARDING_COMPLETED_STORAGE_KEY,
  useOnboarding,
} from './onboarding';

const Host = defineComponent({
  props: {
    ready: Boolean,
  },
  setup(props, { expose }) {
    const activeTab = ref('settings');
    const controller = useOnboarding({
      activeTab,
      ready: toRef(props, 'ready'),
      t: (key) => key,
    });
    expose({ activeTab, ...controller });
    return () => null;
  },
});

async function flushAsyncWork(): Promise<void> {
  await nextTick();
  await flushPromises();
  await nextTick();
}

describe('First-run onboarding tour', () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.body.innerHTML = [
      '<div data-tour="app-status"></div>',
      '<div data-tour="remote-settings"></div>',
    ].join('');
    mocks.driverConfig = undefined;
    mocks.focusHandler = undefined;
    mocks.isFocused.mockReset().mockResolvedValue(false);
    mocks.isVisible.mockReset().mockResolvedValue(false);
    mocks.driver.destroy.mockReset();
    mocks.driver.drive.mockReset();
    mocks.driver.moveTo.mockReset();
  });

  it('only auto-starts after config is ready and window is visible and focused', async () => {
    const wrapper = mount(Host, { props: { ready: false } });
    await flushAsyncWork();
    expect(mocks.driver.drive).not.toHaveBeenCalled();

    await wrapper.setProps({ ready: true });
    await flushAsyncWork();
    expect(mocks.driver.drive).not.toHaveBeenCalled();

    mocks.isFocused.mockResolvedValue(true);
    mocks.isVisible.mockResolvedValue(true);
    mocks.focusHandler?.({ payload: true });
    await flushAsyncWork();
    expect(mocks.driver.drive).toHaveBeenCalledWith(0);
  });

  it('completion marker prevents auto-start but allows manual replay', async () => {
    markOnboardingCompleted();
    expect(isOnboardingCompleted()).toBe(true);
    expect(window.localStorage.getItem(ONBOARDING_COMPLETED_STORAGE_KEY)).toBe('true');

    mocks.isFocused.mockResolvedValue(true);
    mocks.isVisible.mockResolvedValue(true);
    const wrapper = mount(Host, { props: { ready: true } });
    await flushAsyncWork();
    expect(mocks.driver.drive).not.toHaveBeenCalled();

    await (wrapper.vm as unknown as { replayOnboarding: () => Promise<void> }).replayOnboarding();
    expect(mocks.driver.drive).toHaveBeenCalledWith(0);
  });

  it('next step switches corresponding tab before moving Driver.js', async () => {
    mocks.isFocused.mockResolvedValue(true);
    mocks.isVisible.mockResolvedValue(true);
    const wrapper = mount(Host, { props: { ready: true } });
    await flushAsyncWork();

    const config = mocks.driverConfig as {
      onNextClick: (
        element: undefined,
        step: Record<string, never>,
        options: { index: number },
      ) => void;
    };
    config.onNextClick(undefined, {}, { index: 4 });
    await flushAsyncWork();

    expect((wrapper.vm as unknown as { activeTab: string }).activeTab).toBe('remote');
    expect(mocks.driver.moveTo).toHaveBeenCalledWith(5);
  });

  it('skip writes completion marker and destroys onboarding tour', async () => {
    mocks.isFocused.mockResolvedValue(true);
    mocks.isVisible.mockResolvedValue(true);
    mount(Host, { props: { ready: true } });
    await flushAsyncWork();

    const footerButtons = document.createElement('div');
    const config = mocks.driverConfig as {
      onPopoverRender: (popover: { footerButtons: HTMLElement }) => void;
    };
    config.onPopoverRender({ footerButtons });
    footerButtons.querySelector<HTMLButtonElement>('button')?.click();

    expect(isOnboardingCompleted()).toBe(true);
    expect(mocks.driver.destroy).toHaveBeenCalledOnce();
  });

  it('restores previous active tab upon completion', async () => {
    const wrapper = mount(Host, { props: { ready: false } });
    const exposed = wrapper.vm as unknown as {
      activeTab: string;
      replayOnboarding: () => Promise<void>;
    };
    exposed.activeTab = 'updates';
    await exposed.replayOnboarding();

    const config = mocks.driverConfig as { onDoneClick: () => void };
    config.onDoneClick();
    await flushAsyncWork();

    expect(exposed.activeTab).toBe('updates');
    expect(isOnboardingCompleted()).toBe(true);
  });
});
