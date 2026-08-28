package com.chua.playwright.support;

/**
 * Playwright native 调用异常。
 */
public class PlaywrightException extends RuntimeException {

    public PlaywrightException(String message) {
        super(message);
    }

    public PlaywrightException(Throwable cause) {
        super(cause);
    }

    public PlaywrightException(String message, Throwable cause) {
        super(message, cause);
    }
}
