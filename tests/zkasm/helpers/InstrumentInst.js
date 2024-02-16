const fs = require("fs");

class InstrumentInst {
    /**
     * @param {Object} config 
     * @param {string} [config.outputFile] - the file to which output will be written
     */
    constructor(config) {
        this.config = config;
        this.counters = {};
    }

    setup() {
        // `zkevm-proverjs` requires a `setup` function on helper objects.
        console.log("in helper setup");
    }

    /**
     * 
     * @param {Object} ctx 
     * @param {Object} tag 
     * @param {Object} tag.params[0] - expects the identifier as first parameter of
     *   `$${instrumentInst(...)}`
     */
    eval_instrumentInst(ctx, tag) {
        // TODO check num params

        let inst = tag.params[0].varName;
        if (this.counters.hasOwnProperty(inst)) {
            this.counters[inst] += 1;
        } else {
            this.counters[inst] = 1;
        }
    }

    /** Writes counters to `config.outputFile`. */
    eval_writeInstCounters(ctx, tag) {
        // TODO throw error if !this.config.outputFile

        // Generate a data format friendly for deserialization in subsequent steps.
        let counters = [];
        let totalInstCount = 0;
        Object.keys(this.counters).forEach(key => totalInstCount += this.counters[key]);
        Object.keys(this.counters).forEach(key => {
            counters.push({
                name: key,
                count: this.counters[key],
                fraction: (this.counters[key] / totalInstCount).toFixed(4),
            })
        });

        // Sort counters alphabetically to make diff on the written file easier to read.
        counters.sort((a, b) => cmpField("name", a, b));

        let data = JSON.stringify({
            counters: counters
        });
        // TODO handle errors
        fs.writeFileSync(this.config.outputFile, data);
    }
}

/**
 * Can be passed to `Array.prototype.sort` for sorting Object in descending order of their value of
 * `key`.
 * 
 * @param {string} key whose values are compared
 * @param {Object} a 
 * @param {Object} b 
 * @returns {number}
 */
function cmpField(key, a, b) {
    let valA = a[key].toUpperCase();
    let valB = b[key].toUpperCase();
    if (valA < valB) {
        return -1;
    }
    if (valA > valB) {
        return 1;
    }
    return 0;
}

module.exports = InstrumentInst;
