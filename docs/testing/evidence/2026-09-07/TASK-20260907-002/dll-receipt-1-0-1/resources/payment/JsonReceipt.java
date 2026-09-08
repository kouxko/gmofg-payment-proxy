package jp.gmofg.msg.receipt;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.annotations.SerializedName;

import java.util.Date;

import jp.gmofg.common.util.HashUtil;
import jp.gmofg.common.util.StringUtil;
import jp.gmofg.msg.security.AESEncrypter;

/**
 * The type Json receipt.
 */
public class JsonReceipt {

    /**
     * The type Request.
     */
    public static class Request {

        @SerializedName("ConfirmCode")
        private String confirmCode;

        @SerializedName("Password")
        private String password;

        @SerializedName("RequestID")
        private char requestId = 'R';

        @SerializedName("TransactionType")
        private String transactionType;

        @SerializedName("TerminalID")
        private String terminalID;

        @SerializedName("RequestDateTime")
        private final Date requestDateTime = new Date();

        @SerializedName("HashValue")
        private String hashValue;

        /**
         * encryptLength
         */
        @SerializedName("EncryptLength")
        private String encryptLength;

        /**
         * encrypted data
         */
        @SerializedName("GyomuInfo")
        private String gyomuInfo;

        /**
         * Sets encryption info.
         *
         * @param terminalId  the terminal id
         * @param confirmCode the confirm code
         * @param password    the password
         * @param gyomuInfo   the gyomu info
         */
        public void setEncryptionInfo(String terminalId, String confirmCode, String password, GyomuInfo gyomuInfo) {
            this.terminalID = terminalId;
            AESEncrypter aesEncrypter = AESEncrypter.initKey(requestDateTime, terminalId);
            this.confirmCode = aesEncrypter.encData(confirmCode);
            this.password = aesEncrypter.encData(password);
            this.gyomuInfo = aesEncrypter.encData(gyomuInfo.toJsonString());
            this.encryptLength = StringUtil.leftPadZero(String.valueOf(this.gyomuInfo.length()), 6);
            this.hashValue = HashUtil.sha256(this.gyomuInfo);
        }

        /**
         * Sets password.
         *
         * @param password the password
         */
        public void setPassword(String password) {
            this.password = password;
        }

        /**
         * Sets request id.
         *
         * @param requestId the request id
         */
        public void setRequestId(char requestId) {
            this.requestId = requestId;
        }

        /**
         * Sets transaction type.
         *
         * @param transactionType the transaction type
         */
        public void setTransactionType(String transactionType) {
            this.transactionType = transactionType;
        }

        /**
         * To json string string.
         *
         * @return the string
         */
        public String toJsonString() {
            return new GsonBuilder().setDateFormat("yyyyMMddHHmmss").disableHtmlEscaping().create().toJson(this);
        }

    }

    /**
     * The type Response.
     */
    public static class Response {

        @SerializedName("ResponseID")
        private char responseID;

        @SerializedName("TransactionType")
        private String transactionType;

        @SerializedName("ResponseDateTime")
        private String responseDateTime;

        @SerializedName("ErrorCode")
        private String errorCode;

        @SerializedName("ErrorMessage")
        private String errorMessage;

        /**
         * Gets response id.
         *
         * @return the response id
         */
        public char getResponseID() {
            return responseID;
        }

        /**
         * Sets response id.
         *
         * @param responseID the response id
         */
        public void setResponseID(char responseID) {
            this.responseID = responseID;
        }

        /**
         * Gets transaction type.
         *
         * @return the transaction type
         */
        public String getTransactionType() {
            return transactionType;
        }

        /**
         * Sets transaction type.
         *
         * @param transactionType the transaction type
         */
        public void setTransactionType(String transactionType) {
            this.transactionType = transactionType;
        }

        /**
         * Gets response date time.
         *
         * @return the response date time
         */
        public String getResponseDateTime() {
            return responseDateTime;
        }

        /**
         * Sets response date time.
         *
         * @param responseDateTime the response date time
         */
        public void setResponseDateTime(String responseDateTime) {
            this.responseDateTime = responseDateTime;
        }

        /**
         * Gets error code.
         *
         * @return the error code
         */
        public String getErrorCode() {
            return errorCode;
        }

        /**
         * Sets error code.
         *
         * @param errorCode the error code
         */
        public void setErrorCode(String errorCode) {
            this.errorCode = errorCode;
        }

        /**
         * Gets error message.
         *
         * @return the error message
         */
        public String getErrorMessage() {
            return errorMessage;
        }

        /**
         * Sets error message.
         *
         * @param errorMessage the error message
         */
        public void setErrorMessage(String errorMessage) {
            this.errorMessage = errorMessage;
        }
    }

    /**
     * Response from response.
     *
     * @param responseBody the response body
     * @return the response
     */
    public static Response responseFrom(String responseBody) {
        return new Gson().fromJson(responseBody, Response.class);
    }


}
