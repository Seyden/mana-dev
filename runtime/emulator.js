// Buffer polyfill for bare V8 environment (no Node.js, no TextEncoder)
if (typeof globalThis.Buffer === 'undefined') {
  globalThis.Buffer = {
    from: (data, encoding) => {
      if (typeof data === 'string') {
        const arr = [];
        for (let i = 0; i < data.length; i++) arr.push(data.charCodeAt(i) & 0xff);
        return new Uint8Array(arr);
      }
      return new Uint8Array(data);
    },
    alloc: (size) => new Uint8Array(size),
    isBuffer: (obj) => obj instanceof Uint8Array,
    concat: (buffers) => {
      const total = buffers.reduce((acc, b) => acc + b.length, 0);
      const result = new Uint8Array(total);
      let offset = 0;
      for (const b of buffers) { result.set(b, offset); offset += b.length; }
      return result;
    },
  };
}

// axios-stub:axios
var axios = function(config) {
  return Promise.resolve({
    data: '{"success": true}',
    status: 200,
    headers: {},
    config
  });
};
var axios_default = axios;

// src/classes.ts
var globalObject = global;
var GlobalStore = class {
  constructor() {
    this.secure = {};
    this.objects = {};
  }
};
var globalStore = new GlobalStore();
var ManaStore = class {
  constructor(key) {
    this.store = key;
  }
  async get(key) {
    const value = this.store == "ss" ? globalStore.secure[key] : globalStore.objects[key];
    return value ? JSON.parse(value) : null;
  }
  async set(key, value) {
    const v = JSON.stringify(value);
    this.store == "ss" ? globalStore.secure[key] = v : globalStore.objects[key] = v;
  }
  async remove(key) {
    if (this.store == "ss") delete globalStore.secure[key];
    else delete globalStore.objects[key];
  }
  async string(key) {
    const value = await this.get(key);
    if (!value) return null;
    if (typeof value !== "string")
      throw new Error(
        "ObjectStore Type Assertion failed, value is not a string"
      );
    return value;
  }
  async boolean(key) {
    const value = await this.get(key);
    if (!value) return null;
    if (typeof value !== "boolean")
      throw new Error(
        "ObjectStore Type Assertion failed, value is not a boolean"
      );
    return value;
  }
  async number(key) {
    const value = await this.get(key);
    if (!value) return null;
    if (typeof value !== "number")
      throw new Error(
        "ObjectStore Type Assertion failed, value is not a number"
      );
    return value;
  }
  async stringArray(key) {
    const value = await this.get(key);
    if (!value) return null;
    if (typeof value !== "object" || !Array.isArray(value))
      throw new Error(
        "ObjectStore type assertion failed, value is not an array"
      );
    if (!value?.[0]) return value;
    const isValid = value.every((v) => typeof v === "string");
    if (!isValid)
      throw new Error(
        `ObjectStore Type Assertion Failed, Elements of Array are not of type string`
      );
    return value;
  }
};
var NetworkError = class extends Error {
  constructor(name, message, req, res) {
    super(message);
    this.req = req;
    this.res = res;
    this.name = name;
  }
};
var CloudflareError = class extends Error {
  constructor() {
    super("The requested resource is cloudflare protected");
    this.name = "CloudflareError";
  }
};
var NetworkClient = class {
  constructor(builder) {
    // Transformers
    this.requestTransformers = [];
    this.responseTransformers = [];
    this.headers = {};
    this.cookies = [];
    // Rate Limiting
    this.buffer = [];
    this.lastRequestTime = 0;
    this.requestsPerSecond = 999;
    if (builder) {
      this.requestTransformers = builder.requestTransformers;
      this.responseTransformers = builder.responseTransformers;
      this.headers = builder.headers;
      this.cookies = builder.cookies;
      this.timeout = builder.timeout;
      this.statusValidator = builder.statusValidator;
      this.authorizationToken = builder.authorizationToken;
      this.maxRetries = builder.maxRetries;
      this.requestsPerSecond = builder.requestsPerSecond;
    }
  }
  combine(request) {
    const RTX = [...this.requestTransformers];
    if (request.transformRequest) {
      if (typeof request.transformRequest === "function")
        RTX.push(request.transformRequest);
      else RTX.push(...request.transformRequest);
    }
    const RTS = [...this.responseTransformers];
    if (request.transformResponse) {
      if (typeof request.transformResponse === "function")
        RTS.push(request.transformResponse);
      else RTS.push(...request.transformResponse);
    }
    const headers = {
      ...this.headers,
      ...request.headers
    };
    const cookies = [...this.cookies, ...request.cookies ?? []];
    const final = {
      headers,
      cookies,
      url: request.url,
      method: request.method ?? "GET",
      params: request.params,
      body: request.body,
      timeout: request.timeout ?? this.timeout,
      maxRetries: request.maxRetries ?? this.maxRetries,
      transformRequest: RTX,
      transformResponse: RTS,
      validateStatus: request.validateStatus ?? this.statusValidator
    };
    return final;
  }
  async get(url, config) {
    return this.request({ url, method: "GET", ...config });
  }
  async post(url, config) {
    return this.request({ url, method: "POST", ...config });
  }
  async request(request) {
    request = this.combine(request);
    request = await this.factory(
      request,
      request.transformRequest
    );
    if (!this.requestsPerSecond)
      return this.dispatch(
        request,
        request.transformResponse
      );
    return this.rateLimitedRequest(
      () => this.dispatch(
        request,
        request.transformResponse
      )
    );
  }
  async dispatch(request, resTransformers) {
    const cookies = request.cookies?.map((v) => `${v.name}=${v.value}`).join("; ");
    const axResponse = await axios_default({
      method: request.method,
      params: request.params,
      url: request.url,
      headers: {
        ...request.headers,
        Cookie: cookies
      },
      data: request.body,
      validateStatus: () => true
    });
    let response = {
      headers: axResponse.headers,
      status: axResponse.status,
      data: typeof axResponse.data === "string" ? axResponse.data : JSON.stringify(axResponse.data),
      request
    };
    response = await this.factory(
      response,
      resTransformers
    );
    const defaultValidateStatus = (s) => s >= 200 && s < 300;
    const validateStatus = request.validateStatus ?? defaultValidateStatus;
    if (!validateStatus(response.status)) {
      if ([503, 403].includes(response.status) && response.headers["Server"] === "cloudflare")
        throw new CloudflareError();
      const error = new NetworkError(
        "NetworkError",
        `Request failed with status ${response.status}`,
        request,
        response
      );
      switch (response.status) {
        case 400:
          error.message = "Bad Request";
          break;
        case 401:
          error.message = "Unauthorized";
          break;
        case 403:
          error.message = "Forbidden";
          break;
        case 404:
          error.message = "Not Found.\nThe server cannot find the requested resource.";
          break;
        case 405:
          error.message = "Method Not Allowed\nThe request method is known by the server but is not supported by the target resource.";
          break;
        case 410:
          error.message = "Gone.";
          break;
        case 429:
          error.message = "Too Many Requests.";
          break;
        case 431:
          error.message = "Request Header Fields Too Large.\nThe server is unwilling to process the request because its header fields are too large. ";
          break;
        case 500:
          error.message = "Internal Server Error.\nThe server has encountered a situation it does not know how to handle.";
          break;
        case 501:
          error.message = "Not Implemented\nThe request method is not supported by the server and cannot be handled.";
          break;
        case 502:
          error.message = "Bad Gateway\nThis error response means that the server, while working as a gateway to get a response needed to handle the request, got an invalid response.";
          break;
        case 503:
          error.message = "Service Unavailable.The server is not ready to handle the request. Common causes are a server that is down for maintenance or that is overloaded.";
          break;
        case 504:
          error.message = "Gateway Timeout\nThis error response is given when the server is acting as a gateway and cannot get a response in time.";
          break;
      }
      throw error;
    }
    return {
      ...response,
      request
    };
  }
  async factory(r, methods) {
    for (const m of methods) {
      r = await m(r);
    }
    return r;
  }
  rateLimitedRequest(request) {
    return new Promise((resolve, reject) => {
      this.buffer.push({
        request,
        resolve,
        reject
      });
      this.processBuffer();
    });
  }
  processBuffer() {
    if (this.buffer.length === 0) {
      return;
    }
    const now = Date.now();
    const timeSinceLastRequest = now - this.lastRequestTime;
    if (timeSinceLastRequest >= 1e3 / this.requestsPerSecond) {
      const { request, resolve, reject } = this.buffer.shift();
      this.lastRequestTime = now;
      request().then(resolve).catch(reject);
      this.processBuffer();
    } else {
      setTimeout(
        () => this.processBuffer(),
        1e3 / this.requestsPerSecond - timeSinceLastRequest
      );
    }
  }
};
globalObject.ObjectStore = new ManaStore("os");
globalObject.SecureStore = new ManaStore("ss");
globalObject.NetworkClient = NetworkClient;
globalObject.CloudflareError = CloudflareError;
globalObject.NetworkError = NetworkError;

// mana-stub:mana
var BasicAuthenticationUIIdentifier = { EMAIL: 0, USERNAME: 1 };

// src/index.ts
function emulate(v) {
  let target;
  if (typeof v === "function") {
    target = new v();
  } else {
    target = v;
  }
  target.onEnvironmentLoaded?.().catch((err) => {
    console.error("onEnvironmentLoaded", `${err}`);
  });
  return target;
}

function bit(position) {
  return 1 << position;
}

const Intents = {
  preferenceMenuBuilder: 0,
  requiresSetup: 1,
  imageRequestHandler: 2,
  pageLinkResolver: 3,
  libraryPageLinkProvider: 4,
  authenticatable: 5,
  basicAuth: 6,
  basicAuthUsesEmail: 7,
  webviewAuth: 8,
  oauthAuth: 9,
  providesSearch: 10,
  providesSearchForm: 11,
  providesSearchSortOptions: 12,
  chapterEventHandler: 13,
  contentEventHandler: 14,
  librarySyncHandler: 15,
  pageReadHandler: 16,
  progressSyncHandler: 17,
  groupedUpdateFetcher: 18,
  redrawingHandler: 19,
  chaptersInContent: 20,
  providesChapters: 21,
  canHandleURL: 22,
  allowsMultipleInstances: 23,
  requiresAuthenticationToAccessContent: 24
};

function evaluateIntents(target, sourceEnvironment) {
  if (!target) return { flags: 0 };
  let flags = 0;
  let sourceConfig;
  if (target.config) {
    sourceConfig = target.config;
  }
  if (target.getPreferenceMenu) flags |= bit(Intents.preferenceMenuBuilder);
  if (target.getSetupMenu && target.validateSetupForm && target.isRunnerSetup) {
    flags |= bit(Intents.requiresSetup);
  }
  if (target.willRequestImage) flags |= bit(Intents.imageRequestHandler);
  if (target.getSectionsForPage && target.resolvePageSection) {
    flags |= bit(Intents.pageLinkResolver);
  }
  if (target.getLibraryPageLinks) flags |= bit(Intents.libraryPageLinkProvider);
  const sourceAuthenticatable = !!(target.getAuthenticatedUser && target.handleUserSignOut);
  const basicAuthenticatable = !!target.handleBasicAuth;
  const webViewAuthenticatable = !!(target.getWebAuthRequestURL && target.didReceiveSessionCookieFromWebAuthResponse);
  const oAuthAuthenticatable = !!(target.getOAuthRequestURL && target.handleOAuthCallback);
  const authenticatable = basicAuthenticatable || webViewAuthenticatable || oAuthAuthenticatable;
  if (sourceAuthenticatable && authenticatable) {
    flags |= bit(Intents.authenticatable);
    if (basicAuthenticatable) {
      flags |= bit(Intents.basicAuth);
      if (target.BasicAuthenticationUIIdentifier === BasicAuthenticationUIIdentifier.EMAIL) {
        flags |= bit(Intents.basicAuthUsesEmail);
      }
    } else if (webViewAuthenticatable) {
      flags |= bit(Intents.webviewAuth);
    } else if (oAuthAuthenticatable) {
      flags |= bit(Intents.oauthAuth);
    }
  }
  if (target.search) flags |= bit(Intents.providesSearch);
  if (target.getSearchForm) flags |= bit(Intents.providesSearchForm);
  if (target.getSortOptions) {
    flags |= bit(Intents.providesSearchSortOptions);
  }
  if (target.getContent) {
    if (target.onChaptersMarked && target.onChapterRead) {
      flags |= bit(Intents.chapterEventHandler);
    }
    if (target.onContentsAddedToLibrary && target.onContentsRemovedFromLibrary) {
      flags |= bit(Intents.contentEventHandler);
    }
    if (target.syncUserLibrary) flags |= bit(Intents.librarySyncHandler);
    if (target.onPageRead) flags |= bit(Intents.pageReadHandler);
    if (target.getProgressState) flags |= bit(Intents.progressSyncHandler);
    if (target.getGroupedUpdates) flags |= bit(Intents.groupedUpdateFetcher);
    if (target.shouldRedrawImage && target.redrawImageWithSize) {
      flags |= bit(Intents.redrawingHandler);
    }

    let providesChapters = target.getChapterData;
    if (providesChapters) {
      flags |= bit(Intents.providesChapters);
    }
    if (!target.getChapters && providesChapters) {
      flags |= bit(Intents.chaptersInContent);
    }
  }
  if (target.handleURL) flags |= bit(Intents.canHandleURL);
  if (sourceConfig) {
    if (sourceConfig.allowsMultipleInstances) {
      flags |= bit(Intents.allowsMultipleInstances);
    }
    if (sourceConfig.requiresAuthenticationToAccessContent) {
      flags |= bit(Intents.requiresAuthenticationToAccessContent);
    }

  }
  return { flags };
}

function evaluateEnvironment(target) {
  if (!target) return "unknown";

  if (target.getContent 
    && target.search 
    && target.didUpdateLastReadChapter 
    && target.didUpdateStatus 
    && target.getEntryForm 
    && target.didSubmitEntryForm)
    return "tracker";
  
  return "source";
}

var index_default = emulate;
export {
  index_default as default,
  emulate,
  evaluateEnvironment,
  evaluateIntents
};

globalObject.emulate = emulate;
globalObject.evaluateEnvironment = evaluateEnvironment;
globalObject.evaluateIntents = evaluateIntents;
